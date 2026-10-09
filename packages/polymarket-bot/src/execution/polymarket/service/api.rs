//! Synthetic candidates use real canonical market evidence and the shared OMS.
use super::*;
use crate::{
    btc::types::{BtcIntervalMarket, BtcOutcome, OrderbookCheckpoint},
    execution::orders::{EvaluationRequest, Outcome, Receipt},
};
use serde_json::json;
use uuid::Uuid;

fn observe_books(
    books: &BookRegistry,
    market: &BtcIntervalMarket,
) -> (
    Option<OrderbookCheckpoint>,
    Option<OrderbookCheckpoint>,
    chrono::DateTime<Utc>,
) {
    (
        books.checkpoint(&market.up_token_id),
        books.checkpoint(&market.down_token_id),
        Utc::now(),
    )
}

impl ExecutionService {
    pub fn accepts_api_member(&self, member: &str) -> bool {
        self.oms
            .definition
            .execution
            .policies
            .order_execution
            .member_order_types
            .contains_key(member)
    }
    pub async fn api_candidate(
        &self,
        id: Uuid,
        request: &EvaluationRequest,
        market: &BtcIntervalMarket,
    ) -> Result<Candidate> {
        self.freshness
            .time_order(
                super::super::oms::Origin::Api,
                OrderLatencyStage::Evidence,
                async {
                    ensure!(
                        request.candidate.market_id == market.market_id,
                        "market evidence unavailable"
                    );
                    let p = &self.oms.definition.execution.policies;
                    // Execution reads the same canonical gRPC-fed registry as admission and arrival.
                    // Copy both sides under one read lock before choosing the causal reference cutoff.
                    // Historical database checkpoints are not a runtime orderbook dependency.
                    let (up_book, down_book, at) = {
                        let books = self.books.read().await;
                        observe_books(&books, market)
                    };
                    let mut inputs = self
                        .repository
                        .load_point_in_time_inputs(
                            market,
                            at,
                            chrono::Duration::milliseconds(p.evidence.max_chainlink_open_delay_ms),
                            chrono::Duration::milliseconds(p.evidence.max_reference_age_ms),
                            chrono::Duration::milliseconds(p.evidence.max_book_age_ms),
                            None,
                        )
                        .await?;
                    inputs.up_book = up_book;
                    inputs.down_book = down_book;
                    let snapshot = crate::btc::process_runner::build_snapshot(
                        self.oms.owner.process_id,
                        market,
                        at,
                        &inputs,
                        p.sizing.default_share,
                        "synthetic_market_evidence_v1",
                        at,
                        None,
                    );
                    let (outcome, probability) = match request.candidate.outcome {
                        Outcome::Up => (
                            BtcOutcome::Up,
                            request.candidate.synthetic_prediction.probability_up,
                        ),
                        Outcome::Down => (
                            BtcOutcome::Down,
                            Decimal::ONE - request.candidate.synthetic_prediction.probability_up,
                        ),
                    };
                    let mut candidate = Candidate::synthetic(
                        id,
                        request.candidate.member_id.clone(),
                        snapshot,
                        outcome,
                        probability,
                    )?;
                    candidate.api_request_hash = Some(
                        request
                            .identity()
                            .map_err(|_| anyhow::anyhow!("invalid request identity"))?,
                    );
                    Ok(candidate)
                },
            )
            .await
    }
    pub async fn preview_api(&self, candidate: &Candidate, prepare: bool) -> Result<Receipt> {
        self.freshness
            .time_order_outcome(
                candidate.origin(),
                if prepare {
                    OrderLatencyStage::Preparation
                } else {
                    OrderLatencyStage::Preview
                },
                self.preview_api_inner(candidate, prepare),
                |receipt| {
                    if receipt.status == "rejected" {
                        LatencyOutcome::Rejected
                    } else {
                        LatencyOutcome::Completed
                    }
                },
            )
            .await
    }
    async fn preview_api_inner(&self, candidate: &Candidate, prepare: bool) -> Result<Receipt> {
        let live: Option<&dyn ExecutionVenue> = match &self.adapter {
            Adapter::Live(v) => Some(v.as_ref()),
            Adapter::Paper(_) => None,
        };
        let accounting = self
            .freshness
            .time_order(
                candidate.origin(),
                OrderLatencyStage::Accounting,
                accounting(&self.oms, &self.store, &self.repository, live, None),
            )
            .await?;
        let admission_timer = self
            .freshness
            .order_timer(candidate.origin(), OrderLatencyStage::Admission);
        let (intent, policy_results) = {
            let books = self.books.read().await;
            self.oms.admit_report(
                candidate,
                [
                    books
                        .checkpoint(&candidate.snapshot.up_book.token_id)
                        .as_ref(),
                    books
                        .checkpoint(&candidate.snapshot.down_book.token_id)
                        .as_ref(),
                ],
                &accounting,
                Utc::now(),
            )
        };
        admission_timer.finish(if intent.is_ok() {
            LatencyOutcome::Completed
        } else {
            LatencyOutcome::Rejected
        });
        let mut receipt = Receipt {
            version: 1,
            process_id: self.oms.owner.process_id,
            operation_id: candidate.id,
            order_id: None,
            origin: "api".into(),
            config_sha256: self.oms.definition.config_sha256.clone(),
            status: "evaluated".into(),
            policy_results,
            evidence: json!({"observed_at":candidate.snapshot.observed_at,"economic_effects":false}),
            preparation: None,
        };
        match intent {
            Err(error) => {
                receipt.status = "rejected".into();
                let rejection = super::super::AdmissionRejection {
                    reason: error.to_string(),
                    policy_results: receipt.policy_results.clone(),
                };
                if rejection.evidence_unavailable() {
                    return Err(rejection.into());
                }
            }
            Ok(intent) => {
                receipt.evidence = json!({"observed_at":candidate.snapshot.observed_at,"economic_effects":false,"proposed_order":{"token_id":intent.request.token_id,"side":intent.request.side,"order_type":intent.request.order_type,"price":intent.request.price,"size":intent.request.size},"execution":intent.request.metadata.get("polymarket_execution")});
                if prepare {
                    receipt.preparation = Some(match &self.adapter {
                        Adapter::Paper(_) => {
                            json!({"status":"inapplicable","signing":"inapplicable","serialization":"performed"})
                        }
                        Adapter::Live(venue) => {
                            let request = &intent.request;
                            let result = venue
                                .live_order_dry_run(crate::execution::LiveOrderDryRunRequest {
                                    token_id: request.token_id.clone(),
                                    side: request.side,
                                    order_type: Some(request.order_type),
                                    price: request.price,
                                    size: request.size,
                                })
                                .await?;
                            // Allowlist diagnostics; never expose the signed payload, owner or signature.
                            redacted_preparation(&result)
                        }
                    });
                }
            }
        }
        self.freshness.order_event(
            super::super::oms::Origin::Api,
            OrderStage::Preview,
            if receipt.status == "rejected" {
                OrderOutcome::Rejected
            } else {
                OrderOutcome::Accepted
            },
        );
        if let Some(preparation) = &receipt.preparation {
            self.freshness.order_event(
                super::super::oms::Origin::Api,
                OrderStage::Preparation,
                if preparation["status"] == "inapplicable" {
                    OrderOutcome::Inapplicable
                } else {
                    OrderOutcome::Accepted
                },
            );
        }
        Ok(receipt)
    }
}

impl ExecutionService {
    /// Preserve a terminal API policy rejection in the existing order ledger. The recorded
    /// price is observed evidence, explicitly not an approved or submitted execution limit.
    pub(super) fn rejected_api_order(
        &self,
        candidate: &Candidate,
        rejection: &super::super::AdmissionRejection,
    ) -> Result<OrderRecord> {
        let selected = match candidate.outcome {
            BtcOutcome::Up => &candidate.snapshot.up_book,
            BtcOutcome::Down => &candidate.snapshot.down_book,
        };
        let price = selected
            .best_ask
            .filter(|v| *v > Decimal::ZERO)
            .context("execution price evidence unavailable")?;
        let policies = &self.oms.definition.execution.policies;
        let order_type = *policies
            .order_execution
            .member_order_types
            .get(&candidate.member)
            .context("unknown member")?;
        let now = Utc::now();
        Ok(OrderRecord {
            order_id: format!("rejected-{}", candidate.id),
            state: crate::models::OrderState::Rejected,
            created_at: now,
            updated_at: now,
            request: OrderRequest {
                client_order_id: candidate.id,
                process_id: Some(self.oms.owner.process_id),
                market_id: candidate.snapshot.market_id.clone(),
                token_id: selected.token_id.clone(),
                side: crate::models::OrderSide::Buy,
                order_type,
                price,
                size: policies.sizing.default_share,
                metadata: json!({"execution_intent":"entry","run_id":self.oms.owner.run_id,"polymarket_execution":{"origin":"api","mode":self.oms.definition.execution.mode.as_str(),"account_ref":self.oms.definition.execution.account_ref,"config_sha256":self.oms.definition.config_sha256,"candidate_sha256":candidate_identity(candidate)?,"api_request_sha256":candidate.api_request_hash,"intent_prepared":false,"price_kind":"observed_unapproved_ask","policy_results":rejection.policy_results,"starting_collateral_usd":policies.paper_execution.as_ref().map(|p|p.starting_collateral_usd),"rejection_reason":rejection.reason}}),
            },
        })
    }
}

fn redacted_preparation(
    result: &crate::execution::LiveOrderDryRunDiagnostics,
) -> serde_json::Value {
    json!({"status":"performed","signing":"performed","serialization":"performed","signer_matches_client":result.order_signer_matches_authenticated_client,"maker_matches_funder":result.order_maker_matches_configured_funder,"signature_type":result.resolved_signature_type,"owner_redacted":true,"signature_redacted":true,"checked_at":result.checked_at})
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_book_observation_preserves_canonical_lineage_and_missing_evidence() {
        use crate::btc::types::FeedIntegrityStatus;
        use rust_decimal_macros::dec;
        let at = Utc::now() - chrono::Duration::milliseconds(100);
        let market = BtcIntervalMarket {
            event_id: "event".into(),
            event_slug: "event".into(),
            series_slug: "series".into(),
            market_id: "market".into(),
            condition_id: "condition".into(),
            window_start: at - chrono::Duration::seconds(60),
            window_end: at + chrono::Duration::seconds(240),
            up_token_id: "up".into(),
            down_token_id: "down".into(),
            tick_size: dec!(0.01),
            minimum_order_size: Some(dec!(1)),
            resolution_source: "chainlink".into(),
            active: true,
            closed: false,
            accepting_orders: true,
            fees_enabled: true,
            fee_schedule: json!({}),
            raw_payload: json!({}),
        };
        let epoch = Uuid::new_v4();
        let mut books = BookRegistry::new(epoch);
        books.register_market(&market);
        let (up, down, _) = observe_books(&books, &market);
        assert!(up.is_none() && down.is_none());
        for (token, outcome) in [("up", BtcOutcome::Up), ("down", BtcOutcome::Down)] {
            books
                .apply_canonical_snapshot(
                    epoch,
                    &market,
                    token,
                    outcome,
                    at,
                    at,
                    7,
                    None,
                    vec![(dec!(0.39), dec!(100))],
                    vec![(dec!(0.40), dec!(100))],
                )
                .unwrap();
        }
        let (up, down, cutoff) = observe_books(&books, &market);
        for book in [up.unwrap(), down.unwrap()] {
            assert_eq!(book.market_id, market.market_id);
            assert_eq!(book.connection_id, epoch);
            assert_eq!(book.source_timestamp, at);
            assert_eq!(book.received_at, at);
            assert!(book.observed_at <= cutoff);
            assert_eq!(book.ingest_sequence, 7);
            assert_eq!(book.best_ask, Some(dec!(0.40)));
            assert_eq!(book.asks[0].size, dec!(100));
            assert_eq!(book.integrity_status, FeedIntegrityStatus::Ok);
        }
        // Never turn quarantined evidence into a healthy snapshot at the API boundary.
        books.quarantine(FeedIntegrityStatus::PreSnapshot);
        let (up, down, _) = observe_books(&books, &market);
        assert_eq!(
            up.unwrap().integrity_status,
            FeedIntegrityStatus::PreSnapshot
        );
        assert_eq!(
            down.unwrap().integrity_status,
            FeedIntegrityStatus::PreSnapshot
        );
    }

    #[tokio::test]
    async fn rejected_api_attempt_is_non_executable_and_retains_durable_identity() {
        let (oms, mut candidate, _, _) = crate::execution::polymarket::tests::fixture("paper");
        candidate.api_request_hash = Some("request-hash".into());
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@localhost/unused")
            .unwrap();
        let service = ExecutionService::paper(
            oms.definition,
            oms.owner,
            Store::from_pool(pool.clone()),
            BtcRepository::from_pool(pool),
            Arc::new(RwLock::new(BookRegistry::new(Uuid::new_v4()))),
        )
        .unwrap();
        let rejection = crate::execution::polymarket::AdmissionRejection {
            reason: "insufficient_collateral".into(),
            policy_results: json!([{"name":"collateral","status":"rejected"}]),
        };
        let order = service.rejected_api_order(&candidate, &rejection).unwrap();
        assert_eq!(order.state, crate::models::OrderState::Rejected);
        assert_eq!(order.request.client_order_id, candidate.id);
        assert_eq!(
            order.request.metadata["polymarket_execution"]["intent_prepared"],
            false
        );
        assert_eq!(
            order.request.metadata["polymarket_execution"]["api_request_sha256"],
            "request-hash"
        );
        assert_eq!(
            crate::execution::polymarket::lifecycle::resolution(&order, &[], true).unwrap(),
            Resolution::DefinitiveNonexecution
        );
        candidate.snapshot.up_book.best_ask = None;
        assert!(service.rejected_api_order(&candidate, &rejection).is_err());
    }
    #[test]
    fn preparation_diagnostics_cannot_return_signed_or_credential_payloads() {
        let diagnostics = crate::execution::LiveOrderDryRunDiagnostics {
            mode: "live".into(),
            clob_api_base_url: "SECRET_URL".into(),
            signer_address: Some("SECRET_ADDRESS".into()),
            configured_funder_address: None,
            configured_signature_type: None,
            resolved_signature_type: Some("POLY_1271".into()),
            authenticated_client_address: None,
            order_signer: None,
            order_maker: None,
            order_signature_type: None,
            order_signer_matches_authenticated_client: Some(true),
            order_signer_matches_configured_funder: None,
            order_maker_matches_configured_funder: Some(true),
            owner_redacted: false,
            signature_redacted: false,
            signed_order: json!({"signature":"SECRET_SIGNATURE","owner":"SECRET_OWNER","private_key":"SECRET_KEY"}),
            checked_at: Utc::now(),
        };
        let output = redacted_preparation(&diagnostics);
        assert!(!output.to_string().contains("SECRET"));
        assert!(output.get("signed_order").is_none());
        assert_eq!(output["owner_redacted"], true);
        assert_eq!(output["signature_redacted"], true);
    }
}
