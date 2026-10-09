//! Synthetic candidates use real canonical market evidence and the shared OMS.
use super::*;
use crate::{
    btc::types::{BtcIntervalMarket, BtcOutcome},
    execution::orders::{EvaluationRequest, Outcome, Receipt},
};
use serde_json::json;
use uuid::Uuid;

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
        ensure!(
            request.candidate.market_id == market.market_id,
            "market evidence unavailable"
        );
        let p = &self.oms.definition.execution.policies;
        let at = Utc::now();
        let epoch = self.books.read().await.connection_id();
        let inputs = self
            .repository
            .load_point_in_time_inputs(
                market,
                at,
                chrono::Duration::milliseconds(p.evidence.max_chainlink_open_delay_ms),
                chrono::Duration::milliseconds(p.evidence.max_reference_age_ms),
                chrono::Duration::milliseconds(p.evidence.max_book_age_ms),
                Some(epoch),
            )
            .await?;
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
    }
    pub async fn preview_api(&self, candidate: &Candidate, prepare: bool) -> Result<Receipt> {
        let live: Option<&dyn ExecutionVenue> = match &self.adapter {
            Adapter::Live(v) => Some(v.as_ref()),
            Adapter::Paper(_) => None,
        };
        let accounting = accounting(&self.oms, &self.store, &self.repository, live, None).await?;
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
