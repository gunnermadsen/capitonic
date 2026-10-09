//! Transaction boundaries for admitted orders. Uses the existing order/fill identity ledger.
use super::*;

pub(crate) struct ExecutionLease {
    pub transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Store {
    pub async fn execution_order_evidence(
        &self,
        process_id: Uuid,
        order_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        sqlx::query_scalar("SELECT jsonb_build_object('resolution',o.raw_payload #>> '{execution_resolution}','settlement',(SELECT jsonb_build_object('settlement_id',s.settlement_id,'execution_mode',s.execution_mode,'filled_size',s.filled_size,'entry_notional',s.entry_notional,'entry_fees',s.entry_fees,'payout',s.payout,'net_pnl',s.net_pnl,'credit_status',s.credit_status,'credited_at',s.credited_at,'resolved_at',s.official_resolution_received_at) FROM polymarket.btc_paper_settlement_ledger s WHERE s.process_id=o.process_id AND s.order_id=o.order_id LIMIT 1)) FROM polymarket.orders o WHERE o.process_id=$1 AND o.order_id=$2")
            .bind(process_id).bind(order_id).fetch_optional(&self.pool).await.map_err(Into::into)
    }

    pub async fn execution_order(
        &self,
        process_id: Uuid,
        order_id: &str,
    ) -> Result<Option<OrderRecord>> {
        sqlx::query_as::<_,OrderDbRow>("SELECT order_id,state,raw_payload,created_at,updated_at FROM polymarket.orders WHERE process_id=$1 AND order_id=$2")
            .bind(process_id).bind(order_id).fetch_optional(&self.pool).await?.map(order_from_db_row).transpose()
    }

    /// All new-path requests sharing an account serialize admission through this transaction.
    /// The lock survives awaited transport work and is released automatically on connection loss.
    pub(crate) async fn execution_lease(&self, account: &str) -> Result<ExecutionLease> {
        // Leave a connection for the existing Store/venue readers while account leases wait.
        anyhow::ensure!(
            self.pool.options().get_max_connections() > 1,
            "order execution requires a pool with at least two connections"
        );
        let permit = self.order_admission_slots.clone().acquire_owned().await?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("polymarket-order-account:{account}"))
            .execute(&mut *transaction)
            .await?;
        Ok(ExecutionLease {
            transaction,
            _permit: permit,
        })
    }

    pub async fn execution_fills(&self, order: &OrderRecord) -> Result<Vec<FillRecord>> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT raw_payload FROM polymarket.fills WHERE order_id=$1 AND process_id=$2 ORDER BY timestamp_utc, fill_id LIMIT 2049"
        ).bind(&order.order_id).bind(order.request.process_id).fetch_all(&self.pool).await?;
        anyhow::ensure!(
            rows.len() <= 2048,
            "order fill evidence exceeds supported bound"
        );
        rows.into_iter()
            .map(|row| serde_json::from_value(row).map_err(Into::into))
            .collect()
    }

    pub(crate) async fn execution_account_processes(
        &self,
        process_id: Uuid,
        account: Option<&str>,
    ) -> Result<Vec<Uuid>> {
        match account {
            None => Ok(vec![process_id]),
            Some(account) => Ok(sqlx::query_scalar("SELECT process_id FROM polymarket.trading_processes WHERE lower(config #>> '{execution,account_ref}')=lower($1) AND config #>> '{execution,mode}'='live' UNION SELECT process_id FROM polymarket.orders WHERE lower(raw_payload #>> '{request,metadata,polymarket_execution,account_ref}')=lower($1) AND (state NOT IN ('filled','rejected','cancelled','expired') OR (state IN ('cancelled','expired') AND COALESCE(raw_payload #>> '{execution_resolution}', '') NOT IN ('definitive_nonexecution','executed'))) ORDER BY process_id")
                .bind(account).fetch_all(&self.pool).await?),
        }
    }

    pub(crate) async fn execution_opportunity_occupied(
        &self,
        process_id: Uuid,
        market: &str,
    ) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM polymarket.orders o WHERE o.process_id=$1 AND o.market_id=$2 AND (o.state NOT IN ('rejected','cancelled','expired') OR EXISTS(SELECT 1 FROM polymarket.fills f WHERE f.process_id=$1 AND f.order_id=o.order_id) OR (o.state IN ('cancelled','expired') AND o.raw_payload #>> '{request,metadata,polymarket_execution,mode}'='live' AND COALESCE(o.raw_payload #>> '{execution_resolution}', '') <> 'definitive_nonexecution')))")
            .bind(process_id).bind(market).fetch_one(&self.pool).await?)
    }

    pub(crate) async fn execution_reserved_orders(
        &self,
        process_id: Uuid,
    ) -> Result<Vec<OrderRecord>> {
        let rows=sqlx::query_as::<_,OrderDbRow>("SELECT order_id,state,raw_payload,created_at,updated_at FROM polymarket.orders WHERE process_id=$1 AND (state NOT IN ('filled','rejected','cancelled','expired') OR (state IN ('cancelled','expired') AND raw_payload #>> '{request,metadata,polymarket_execution,mode}'='live' AND COALESCE(raw_payload #>> '{execution_resolution}', '') NOT IN ('definitive_nonexecution','executed'))) ORDER BY created_at,order_id LIMIT 1001")
            .bind(process_id).fetch_all(&self.pool).await?;
        anyhow::ensure!(
            rows.len() <= 1000,
            "unresolved order evidence exceeds existing bound"
        );
        rows.into_iter().map(order_from_db_row).collect()
    }

    pub(crate) async fn confirm_execution_resolution(
        &self,
        order: &OrderRecord,
        resolution: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE polymarket.orders SET raw_payload=jsonb_set(raw_payload,'{execution_resolution}',to_jsonb($4::text),true) WHERE client_order_id=$1 AND process_id=$2 AND state=$3 AND updated_at=$5")
            .bind(order.request.client_order_id).bind(order.request.process_id).bind(serialized_name(&order.state)?)
            .bind(resolution).bind(order.updated_at).execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn execution_open_markets(&self, process_id: Uuid) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar("SELECT DISTINCT o.market_id FROM polymarket.orders o WHERE o.process_id=$1 AND (o.state NOT IN ('filled','cancelled','rejected','expired') OR (EXISTS(SELECT 1 FROM polymarket.fills f WHERE f.process_id=$1 AND f.order_id=o.order_id) AND NOT EXISTS(SELECT 1 FROM polymarket.btc_paper_settlement_ledger s WHERE s.process_id=$1 AND s.order_id=o.order_id AND s.credit_status='credited'))) LIMIT 257")
            .bind(process_id).fetch_all(&self.pool).await?)
    }

    /// Reconstruct process capital across runs; restarting never creates a fresh paper balance.
    pub(crate) async fn paper_capital(
        &self,
        process_id: Uuid,
        configured_seed: Decimal,
    ) -> Result<Decimal> {
        let seed: Option<String> = sqlx::query_scalar("SELECT raw_payload #>> '{request,metadata,polymarket_execution,starting_collateral_usd}' FROM polymarket.orders WHERE process_id=$1 AND raw_payload #>> '{request,metadata,polymarket_execution,mode}'='paper' ORDER BY created_at, order_id LIMIT 1")
            .bind(process_id).fetch_optional(&self.pool).await?.flatten();
        let seed = seed
            .map(|v| Decimal::from_str_exact(&v))
            .transpose()?
            .unwrap_or(configured_seed);
        let debit: Decimal = sqlx::query_scalar("SELECT COALESCE(SUM(price*size+fee),0)::numeric FROM polymarket.fills WHERE process_id=$1 AND source='paper'")
            .bind(process_id).fetch_one(&self.pool).await?;
        let credit: Decimal = sqlx::query_scalar("SELECT COALESCE(SUM(payout),0)::numeric FROM polymarket.btc_paper_settlement_ledger WHERE process_id=$1 AND execution_mode='paper' AND credit_status='credited'")
            .bind(process_id).fetch_one(&self.pool).await?;
        let available = seed - debit + credit;
        anyhow::ensure!(
            available >= Decimal::ZERO,
            "paper capital evidence is inconsistent"
        );
        Ok(available)
    }
}

#[async_trait::async_trait]
pub(crate) trait PaperCommit: Send {
    async fn commit(self: Box<Self>, order: &OrderRecord, fills: &[FillRecord]) -> Result<()>;
}
#[async_trait::async_trait]
impl PaperCommit for ExecutionLease {
    async fn commit(self: Box<Self>, order: &OrderRecord, fills: &[FillRecord]) -> Result<()> {
        self.commit_paper(order, fills).await
    }
}

impl ExecutionLease {
    /// An atomic order+fills commit, sharing the legacy canonical fill validation implementation.
    pub(crate) async fn commit_paper(
        mut self,
        order: &OrderRecord,
        fills: &[FillRecord],
    ) -> Result<()> {
        let result = sqlx::query(INSERT_ORDER_SQL)
            .bind(&order.order_id)
            .bind(order.request.client_order_id)
            .bind(order.request.process_id)
            .bind(order.created_at)
            .bind(order.updated_at)
            .bind(&order.request.market_id)
            .bind(&order.request.token_id)
            .bind(serialized_name(&order.request.side)?)
            .bind(serialized_name(&order.request.order_type)?)
            .bind(order.request.price)
            .bind(order.request.size)
            .bind(serialized_name(&order.state)?)
            .bind(serde_json::to_value(order)?)
            .execute(&mut *self.transaction)
            .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "paper order identity already exists; recover before retry"
        );
        for fill in fills {
            Store::insert_fill_in_transaction(&mut self.transaction, fill).await?;
        }
        self.transaction
            .commit()
            .await
            .context("paper commit outcome uncertain; recover by client_order_id")
    }

    pub(crate) async fn release(self) -> Result<()> {
        self.transaction.commit().await?;
        Ok(())
    }
}
