//! Read-only environment ownership evidence for a shared wallet. Foreign evidence is never
//! imported into local orders, fills or settlement ownership.
use super::{
    account_owned_fills_for_reconciliation, account_owned_orders_for_reconciliation,
    link_process_owned_trades,
};
use crate::store::{AccountLiveFillEvidence, AccountTrade, LiveAccountPositionEvidence, Store};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    time::Duration,
};
use uuid::Uuid;

const MAX_IDENTITIES: usize = 4_000;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const PEER_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub struct OwnershipConfig {
    pub environment: Option<String>,
    pub peer_environment: Option<String>,
    pub peer_url: Option<String>,
    pub peer_token: Option<String>,
}
impl fmt::Debug for OwnershipConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnershipConfig")
            .field("environment", &self.environment)
            .field("peer_environment", &self.peer_environment)
            .field("peer_url", &self.peer_url)
            .field(
                "peer_token",
                &self.peer_token.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}
impl OwnershipConfig {
    pub fn from_env() -> Result<Self> {
        let read = |name| std::env::var(name).ok().filter(|s| !s.trim().is_empty());
        let config = Self {
            environment: read("POLYMARKET_RECONCILIATION_ENVIRONMENT"),
            peer_environment: read("POLYMARKET_RECONCILIATION_PEER_ENVIRONMENT"),
            peer_url: read("POLYMARKET_RECONCILIATION_PEER_URL"),
            peer_token: read("POLYMARKET_RECONCILIATION_PEER_TOKEN"),
        };
        config.validate()?;
        Ok(config)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        for name in [&self.environment, &self.peer_environment]
            .into_iter()
            .flatten()
        {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
            {
                bail!("reconciliation environment must be a bounded identifier");
            }
        }
        if self.peer_environment.is_some() || self.peer_url.is_some() || self.peer_token.is_some() {
            if self.environment.is_none()
                || self.peer_environment.is_none()
                || self.peer_url.is_none()
                || self
                    .peer_token
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
            {
                bail!("reconciliation peer requires local/peer environments, URL and bearer token");
            }
            if self.environment == self.peer_environment {
                bail!("reconciliation peer must be a different environment");
            }
            let url = reqwest::Url::parse(self.peer_url.as_deref().unwrap())?;
            if !matches!(url.scheme(), "https" | "http")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!("reconciliation peer URL must be HTTP(S), without credentials, query or fragment");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnershipRequest {
    pub request_id: Uuid,
    pub account_address: String,
    pub account_ref: String,
    pub chain_id: u64,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub venue_order_ids: Vec<String>,
    pub transaction_hashes: Vec<String>,
    pub token_ids: Vec<String>,
}
impl OwnershipRequest {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.chain_id != 137
            || self.account_ref.trim().is_empty()
            || self.account_ref.len() > 128
            || self.account_address.len() != 42
            || !self.account_address.starts_with("0x")
            || !self.account_address[2..]
                .bytes()
                .all(|c| c.is_ascii_hexdigit())
        {
            bail!("invalid reconciliation ownership identity");
        }
        if self.window_start > self.window_end
            || self.window_end - self.window_start > chrono::Duration::days(31)
            || self.window_end > Utc::now() + crate::execution::LIVE_EXTERNAL_EVENT_CLOCK_SKEW
        {
            bail!("invalid reconciliation ownership window");
        }
        for ids in [
            &self.venue_order_ids,
            &self.transaction_hashes,
            &self.token_ids,
        ] {
            if ids.len() > MAX_IDENTITIES || ids.iter().any(|id| id.is_empty() || id.len() > 256) {
                bail!("reconciliation ownership identity limit exceeded");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipEvidence {
    pub request_id: Uuid,
    pub environment: String,
    pub account_address: String,
    pub account_ref: String,
    pub chain_id: u64,
    pub observed_at: DateTime<Utc>,
    pub process_ids: Vec<Uuid>,
    pub(crate) orders: HashMap<String, String>,
    pub(crate) order_process_ids: HashMap<String, Uuid>,
    pub(crate) fills: Vec<AccountLiveFillEvidence>,
    pub(crate) positions: HashMap<String, LiveAccountPositionEvidence>,
    pub(crate) token_lineage: HashSet<String>,
}

/// Local SELECTs only: this endpoint never calls its peer or invokes reconciliation.
pub(crate) async fn local_evidence(
    store: &Store,
    environment: &str,
    request: &OwnershipRequest,
) -> Result<OwnershipEvidence> {
    request.validate()?;
    let observed_at = Utc::now();
    let orders = account_owned_orders_for_reconciliation(
        store,
        &request.account_ref,
        &request.venue_order_ids,
    )
    .await?;
    let fills = account_owned_fills_for_reconciliation(
        store,
        &request.account_ref,
        &request.transaction_hashes,
        request.window_start,
        request.window_end,
    )
    .await?;
    let mut order_ids: Vec<_> = orders
        .values()
        .cloned()
        .chain(fills.iter().map(|fill| fill.order_id.clone()))
        .collect();
    order_ids.sort_unstable();
    order_ids.dedup();
    let order_process_ids = store.reconciliation_order_process_ids(&order_ids).await?;
    Ok(OwnershipEvidence {
        request_id: request.request_id,
        environment: environment.to_owned(),
        account_address: request.account_address.clone(),
        account_ref: request.account_ref.clone(),
        chain_id: request.chain_id,
        observed_at,
        orders,
        fills,
        order_process_ids,
        process_ids: store
            .reconciliation_account_process_ids(&request.account_ref)
            .await?,
        token_lineage: store
            .reconciliation_account_token_lineage(&request.account_ref, &request.token_ids)
            .await?,
        positions: store
            .live_account_position_sizes(&request.account_ref)
            .await?,
    })
}

#[derive(Clone)]
pub(crate) struct OwnershipClient {
    config: OwnershipConfig,
    client: reqwest::Client,
}
impl OwnershipClient {
    pub(crate) fn new(config: &OwnershipConfig) -> Result<Option<Self>> {
        config.validate()?;
        if config.peer_url.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            config: config.clone(),
            client: reqwest::Client::builder()
                .timeout(PEER_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        }))
    }
    pub(crate) fn environment(&self) -> &str {
        self.config.environment.as_deref().unwrap()
    }
    pub(crate) fn peer_environment(&self) -> &str {
        self.config.peer_environment.as_deref().unwrap()
    }
    pub(crate) async fn fetch(&self, request: &OwnershipRequest) -> Result<OwnershipEvidence> {
        request.validate()?;
        let started = Utc::now();
        let mut response = self
            .client
            .post(self.config.peer_url.as_deref().unwrap())
            .bearer_auth(self.config.peer_token.as_deref().unwrap())
            .json(request)
            .send()
            .await
            .context("reconciliation_peer_unavailable")?
            .error_for_status()
            .context("reconciliation_peer_unavailable")?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("reconciliation_peer_unavailable")?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                bail!("reconciliation_peer_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        let evidence: OwnershipEvidence =
            serde_json::from_slice(&bytes).context("invalid_reconciliation_peer_evidence")?;
        evidence.validate(request, self.peer_environment(), started, Utc::now())?;
        Ok(evidence)
    }
}

impl OwnershipEvidence {
    fn validate(
        &self,
        request: &OwnershipRequest,
        environment: &str,
        started: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if self.request_id != request.request_id
            || self.environment != environment
            || !self
                .account_address
                .eq_ignore_ascii_case(&request.account_address)
            || self.account_ref != request.account_ref
            || self.chain_id != request.chain_id
        {
            bail!("reconciliation_peer_identity_mismatch");
        }
        if self.observed_at < started - chrono::Duration::seconds(5)
            || self.observed_at > now + chrono::Duration::seconds(5)
        {
            bail!("reconciliation_peer_evidence_stale");
        }
        if self.process_ids.len() > MAX_IDENTITIES
            || self.orders.len() > MAX_IDENTITIES
            || self.fills.len() > MAX_IDENTITIES
            || self.positions.len() > MAX_IDENTITIES
            || self.token_lineage.len() > MAX_IDENTITIES
            || self.order_process_ids.len() > MAX_IDENTITIES * 2
        {
            bail!("reconciliation_peer_evidence_limit_exceeded");
        }
        let processes: HashSet<_> = self.process_ids.iter().copied().collect();
        if processes.len() != self.process_ids.len() || processes.contains(&Uuid::nil()) {
            bail!("reconciliation_peer_ownership_conflict");
        }
        for order_id in self
            .orders
            .values()
            .chain(self.fills.iter().map(|f| &f.order_id))
        {
            if !self
                .order_process_ids
                .get(order_id)
                .is_some_and(|id| processes.contains(id))
            {
                bail!("reconciliation_peer_missing_process_owner");
            }
        }
        if self
            .orders
            .keys()
            .any(|id| !request.venue_order_ids.contains(id))
            || self
                .fills
                .iter()
                .any(|f| !request.transaction_hashes.contains(&f.transaction_hash))
        {
            bail!("reconciliation_peer_unrequested_evidence");
        }
        if self
            .token_lineage
            .iter()
            .any(|token| !request.token_ids.contains(token))
        {
            bail!("reconciliation_peer_unrequested_evidence");
        }
        if self
            .positions
            .values()
            .any(|p| p.size < Decimal::ZERO || p.credited_zero_payout_size < Decimal::ZERO)
        {
            bail!("reconciliation_peer_invalid_position");
        }
        Ok(())
    }
    pub(crate) fn ensure_disjoint(
        &self,
        local_process_ids: &[Uuid],
        local_orders: &HashMap<String, String>,
    ) -> Result<()> {
        if self
            .process_ids
            .iter()
            .any(|id| local_process_ids.contains(id))
            || self.orders.keys().any(|id| local_orders.contains_key(id))
        {
            bail!("reconciliation_environment_ownership_conflict");
        }
        Ok(())
    }
    pub(crate) fn foreign_trades(&self, trades: &[AccountTrade]) -> Result<HashSet<Uuid>> {
        let mut foreign = trades.to_vec();
        for trade in &mut foreign {
            trade.linked_order_id = None;
        }
        link_process_owned_trades(&mut foreign, &self.orders, &self.fills);
        let mut ids = HashSet::new();
        for (local, peer) in trades.iter().zip(foreign) {
            if peer.linked_order_id.is_some() {
                if local.linked_order_id.is_some() {
                    bail!("reconciliation_environment_ownership_conflict");
                }
                ids.insert(local.account_trade_id);
            }
        }
        Ok(ids)
    }
    pub(crate) fn merge_positions(
        &self,
        local: &mut HashMap<String, LiveAccountPositionEvidence>,
    ) -> Result<()> {
        for (token, peer) in &self.positions {
            if let Some(position) = local.get_mut(token) {
                position.size = position
                    .size
                    .checked_add(peer.size)
                    .context("combined wallet position overflow")?;
                position.credited_zero_payout_size = position
                    .credited_zero_payout_size
                    .checked_add(peer.credited_zero_payout_size)
                    .context("combined wallet settlement overflow")?;
                position.oldest_fill_at = position.oldest_fill_at.min(peer.oldest_fill_at);
                position.market_window_end = position.market_window_end.max(peer.market_window_end);
            } else {
                local.insert(token.clone(), peer.clone());
            }
        }
        Ok(())
    }
    pub(crate) fn has_token_lineage(&self, token: &str) -> bool {
        self.positions.contains_key(token) || self.token_lineage.contains(token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentOwnershipStatus {
    pub environment: String,
    pub peer_environment: String,
    pub peer_evidence_status: String,
    pub confirmed_foreign_trades: usize,
    pub confirmed_foreign_open_orders: usize,
    pub ambiguous_accounting_events: usize,
}

#[cfg(test)]
mod tests;
