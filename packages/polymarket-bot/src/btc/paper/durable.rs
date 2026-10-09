//! The new OMS uses the same arrival simulator without a second in-memory economic ledger.
use super::*;
use crate::btc::execution_freshness::{LatencyOutcome, OrderLatencyStage};
use crate::execution::polymarket::oms::Origin;
use crate::store::PaperCommit;

impl PaperVenue {
    pub(crate) fn with_order_metrics(mut self, metrics: Arc<ExecutionFreshnessMetrics>) -> Self {
        self.freshness_metrics = Some(metrics);
        self
    }

    pub(crate) fn for_order_management(
        registry: Arc<RwLock<BookRegistry>>,
        config: PaperVenueConfig,
        max_depth_participation: Decimal,
    ) -> Result<Self> {
        // The OMS validates reference and both books for both modes at the adjacent boundary.
        Self::new_inner(
            registry,
            config,
            max_depth_participation,
            EffectiveProcessExecutionConfig::default(),
            None,
        )
    }

    pub(crate) async fn execute_durable(
        &self,
        mut request: OrderRequest,
        collateral: Decimal,
        lease: Box<dyn PaperCommit>,
        guard: Arc<dyn crate::execution::LivePrePostGuard>,
    ) -> Result<OrderRecord> {
        let submitted_at = Utc::now();
        let origin = if request
            .metadata
            .pointer("/polymarket_execution/origin")
            .and_then(|v| v.as_str())
            == Some("api")
        {
            Origin::Api
        } else {
            Origin::Umr
        };
        let timer = self
            .freshness_metrics
            .as_ref()
            .map(|m| m.order_timer(origin, OrderLatencyStage::PaperArrival));
        tokio::time::sleep(self.config.arrival_latency).await;
        if let Some(timer) = timer {
            timer.finish(LatencyOutcome::Completed);
        }
        let arrival_at = Utc::now();
        let order_id = format!("paper-{}", request.client_order_id);
        let execution = match guard.validate_pre_post(&request).await? {
            Some(reason) => paper_reject(serde_json::json!({}), reason.as_str()),
            None => {
                self.execute_at_arrival(
                    &order_id,
                    &request,
                    None,
                    None,
                    submitted_at,
                    arrival_at,
                    collateral,
                    self.config.arrival_latency,
                    self.config.visible_depth_haircut,
                    self.config.max_book_age,
                    true,
                    None,
                )
                .await
            }
        };
        request.metadata = merge_json(request.metadata, execution.metadata);
        let order = OrderRecord {
            order_id,
            request,
            state: execution.state,
            created_at: submitted_at,
            updated_at: arrival_at,
        };
        crate::execution::polymarket::lifecycle::validate_fills(&order, &execution.fills)?;
        // No memory debit precedes this commit. A lost response is resolved from the same ledger.
        let timer = self
            .freshness_metrics
            .as_ref()
            .map(|m| m.order_timer(origin, OrderLatencyStage::Persistence));
        let result = lease.commit(&order, &execution.fills).await;
        if let Some(timer) = timer {
            timer.finish(if result.is_ok() {
                LatencyOutcome::Completed
            } else {
                LatencyOutcome::Error
            });
        }
        result?;
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc::types::{BtcIntervalMarket, BtcOutcome};
    use rust_decimal_macros::dec;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Guard {
        deny: bool,
    }
    impl crate::execution::live_pre_post_guard_sealed::Sealed for Guard {}
    #[async_trait]
    impl crate::execution::LivePrePostGuard for Guard {
        async fn validate_pre_post(
            &self,
            _: &OrderRequest,
        ) -> Result<Option<crate::execution::LiveExecutionGateReason>> {
            Ok(self
                .deny
                .then_some(crate::execution::LiveExecutionGateReason::OrderbookFreshness))
        }
    }
    type Committed = Arc<Mutex<Option<(OrderRecord, Vec<FillRecord>)>>>;
    struct Commit {
        fault: u8,
        committed: Committed,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl PaperCommit for Commit {
        async fn commit(self: Box<Self>, order: &OrderRecord, fills: &[FillRecord]) -> Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fault == 1 {
                anyhow::bail!("injected failure before commit");
            }
            *self.committed.lock().await = Some((order.clone(), fills.to_vec()));
            if self.fault == 2 {
                anyhow::bail!("injected lost commit response");
            }
            Ok(())
        }
    }
    fn fixture() -> (PaperVenue, OrderRequest) {
        let now = Utc::now();
        let market = BtcIntervalMarket {
            event_id: "event".into(),
            event_slug: "event".into(),
            series_slug: "series".into(),
            market_id: "market".into(),
            condition_id: "condition".into(),
            window_start: now - chrono::Duration::seconds(60),
            window_end: now + chrono::Duration::seconds(240),
            up_token_id: "up".into(),
            down_token_id: "down".into(),
            tick_size: dec!(0.01),
            minimum_order_size: Some(dec!(1)),
            resolution_source: "chainlink".into(),
            active: true,
            closed: false,
            accepting_orders: true,
            fees_enabled: true,
            fee_schedule: serde_json::json!({}),
            raw_payload: serde_json::json!({}),
        };
        let mut registry = BookRegistry::new(Uuid::new_v4());
        registry.register_market(&market);
        registry
            .apply_canonical_snapshot(
                registry.connection_id(),
                &market,
                "up",
                BtcOutcome::Up,
                now,
                now,
                1,
                None,
                vec![(dec!(0.39), dec!(100))],
                vec![(dec!(0.40), dec!(100))],
            )
            .unwrap();
        let venue = PaperVenue::for_order_management(
            Arc::new(RwLock::new(registry)),
            PaperVenueConfig {
                arrival_latency: Duration::from_millis(1),
                visible_depth_haircut: dec!(0.8),
                max_book_age: chrono::Duration::seconds(2),
                starting_collateral_usd: dec!(100),
            },
            dec!(0.25),
        )
        .unwrap();
        let request = OrderRequest {
            client_order_id: Uuid::new_v4(),
            process_id: Some(Uuid::new_v4()),
            market_id: "market".into(),
            token_id: "up".into(),
            side: OrderSide::Buy,
            order_type: OrderType::Fok,
            price: dec!(0.40),
            size: dec!(5),
            metadata: serde_json::json!({"dynamic_fee_rate":"0.25"}),
        };
        let metrics = ExecutionFreshnessMetrics::new(request.process_id.unwrap(), "paper", 2000);
        (venue.with_order_metrics(metrics), request)
    }
    #[tokio::test]
    async fn failed_persistence_does_not_apply_a_paper_balance_or_fill() {
        let (venue, request) = fixture();
        let committed = Arc::new(Mutex::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let process_label = format!("process_id=\"{}\"", request.process_id.unwrap());
        let result = venue
            .execute_durable(
                request,
                dec!(100),
                Box::new(Commit {
                    fault: 1,
                    committed: committed.clone(),
                    calls: calls.clone(),
                }),
                Arc::new(Guard { deny: false }),
            )
            .await;
        assert!(result.is_err());
        let text = crate::btc::execution_freshness::prometheus_metrics(&serde_json::json!({}));
        // Actual durable execution, including failed commit, must expose timings.
        assert!(text.lines().any(|l| l
            .starts_with("polymarket_order_stage_duration_seconds_count")
            && l.contains(&process_label)
            && l.contains("stage=\"persistence\",outcome=\"error\"")));
        assert!(text.lines().any(|l| l
            .starts_with("polymarket_order_stage_duration_seconds_count")
            && l.contains(&process_label)
            && l.contains("stage=\"paper_arrival\",outcome=\"completed\"")));
        assert!(committed.lock().await.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(venue.status().await.entry_debits_usd, Decimal::ZERO);
        assert_eq!(venue.status().await.fill_count, 0);
    }
    #[tokio::test]
    async fn lost_commit_response_leaves_complete_recoverable_order_and_fill_evidence() {
        let (venue, request) = fixture();
        let id = request.client_order_id;
        let committed = Arc::new(Mutex::new(None));
        let result = venue
            .execute_durable(
                request,
                dec!(100),
                Box::new(Commit {
                    fault: 2,
                    committed: committed.clone(),
                    calls: Arc::new(AtomicUsize::new(0)),
                }),
                Arc::new(Guard { deny: false }),
            )
            .await;
        assert!(result.is_err());
        let durable = committed.lock().await;
        let (order, fills) = durable.as_ref().unwrap();
        assert_eq!(order.request.client_order_id, id);
        assert_eq!(
            crate::execution::polymarket::lifecycle::resolution(order, fills, true).unwrap(),
            crate::execution::polymarket::lifecycle::Resolution::Executed
        );
        assert_eq!(venue.status().await.entry_debits_usd, Decimal::ZERO);
    }
    #[tokio::test]
    async fn arrival_guard_rejects_without_any_fill_and_recovers_on_next_action() {
        let (venue, request) = fixture();
        let committed = Arc::new(Mutex::new(None));
        let result = venue
            .execute_durable(
                request.clone(),
                dec!(100),
                Box::new(Commit {
                    fault: 0,
                    committed: committed.clone(),
                    calls: Arc::new(AtomicUsize::new(0)),
                }),
                Arc::new(Guard { deny: true }),
            )
            .await
            .unwrap();
        assert_eq!(result.state, OrderState::Rejected);
        assert!(committed.lock().await.as_ref().unwrap().1.is_empty());
        let result = venue
            .execute_durable(
                request,
                dec!(100),
                Box::new(Commit {
                    fault: 0,
                    committed: committed.clone(),
                    calls: Arc::new(AtomicUsize::new(0)),
                }),
                Arc::new(Guard { deny: false }),
            )
            .await
            .unwrap();
        assert_eq!(result.state, OrderState::Filled);
    }
    #[tokio::test]
    async fn persisted_available_capital_is_authoritative_after_restart() {
        let (venue, request) = fixture();
        let committed = Arc::new(Mutex::new(None));
        let result = venue
            .execute_durable(
                request,
                dec!(0.01),
                Box::new(Commit {
                    fault: 0,
                    committed: committed.clone(),
                    calls: Arc::new(AtomicUsize::new(0)),
                }),
                Arc::new(Guard { deny: false }),
            )
            .await
            .unwrap();
        assert_eq!(result.state, OrderState::Rejected);
        assert!(committed.lock().await.as_ref().unwrap().1.is_empty());
    }
    #[tokio::test]
    async fn cash_budget_price_improvement_matches_live_semantics_and_legacy_stays_share_sized() {
        for cash in [false, true] {
            let (venue, mut request) = fixture();
            request.price = dec!(0.50);
            if cash {
                request.metadata["polymarket_execution"] =
                    serde_json::json!({"quantity_semantics":"buy_cash_budget"});
            }
            let committed = Arc::new(Mutex::new(None));
            let order = venue
                .execute_durable(
                    request,
                    dec!(100),
                    Box::new(Commit {
                        fault: 0,
                        committed: committed.clone(),
                        calls: Arc::new(AtomicUsize::new(0)),
                    }),
                    Arc::new(Guard { deny: false }),
                )
                .await
                .unwrap();
            assert_eq!(order.state, OrderState::Filled);
            let data = committed.lock().await;
            let fills = &data.as_ref().unwrap().1;
            let size = fills.iter().map(|f| f.size).sum::<Decimal>();
            assert_eq!(size, if cash { dec!(6.25) } else { dec!(5) });
            assert_eq!(
                fills.iter().map(|f| f.price * f.size).sum::<Decimal>(),
                if cash { dec!(2.5) } else { dec!(2) }
            );
        }
    }
}
