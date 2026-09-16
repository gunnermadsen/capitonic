use anyhow::Result;
use chrono::Utc;
use prometheus_client::{
    encoding::{text::encode, EncodeLabelSet},
    metrics::{family::Family, gauge::Gauge},
    registry::Registry,
};

use crate::{domain::IngesterProfile, persistence::WorkerAllocationRecord};

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct StrategyStateLabels {
    strategy: String,
    desired_state: String,
    observed_state: String,
    health_status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct StrategyLabels {
    strategy: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct WorkerLabels {
    worker_id: String,
}

pub fn render(
    profiles: &[IngesterProfile],
    allocations: &[WorkerAllocationRecord],
    ready: bool,
) -> Result<String> {
    let mut registry = Registry::with_prefix("market_data_ingester");
    let liveness = Gauge::<i64>::default();
    let readiness = Gauge::<i64>::default();
    let strategy_state = Family::<StrategyStateLabels, Gauge<i64>>::default();
    let last_persistence = Family::<StrategyLabels, Gauge<i64>>::default();
    let worker_capacity = Family::<WorkerLabels, Gauge<i64>>::default();
    let worker_allocated = Family::<WorkerLabels, Gauge<i64>>::default();
    let worker_realtime = Family::<WorkerLabels, Gauge<i64>>::default();
    let worker_backfills = Family::<WorkerLabels, Gauge<i64>>::default();
    let realtime_profiles_desired = Gauge::<i64>::default();
    let realtime_profiles_owned = Gauge::<i64>::default();
    let realtime_profiles_healthy = Gauge::<i64>::default();
    let worker_realtime_slots_total = Gauge::<i64>::default();
    let worker_realtime_slots_available = Gauge::<i64>::default();
    let worker_capacity_units_total = Gauge::<i64>::default();
    let worker_capacity_units_available = Gauge::<i64>::default();
    let backfill_capacity_units_active = Gauge::<i64>::default();

    registry.register(
        "liveness",
        "Whether the market-data-ingester HTTP process is live.",
        liveness.clone(),
    );
    registry.register(
        "worker_capacity_units",
        "Configured worker allocation capacity.",
        worker_capacity.clone(),
    );
    registry.register(
        "worker_allocated_units",
        "Currently leased worker allocation units.",
        worker_allocated.clone(),
    );
    registry.register(
        "worker_realtime_leases",
        "Current realtime leases owned by a worker.",
        worker_realtime.clone(),
    );
    registry.register(
        "worker_backfill_leases",
        "Current backfill leases owned by a worker.",
        worker_backfills.clone(),
    );
    registry.register(
        "readiness",
        "Whether the strategy supervisor has completed startup reconciliation.",
        readiness.clone(),
    );
    registry.register(
        "strategy_state",
        "Current desired, observed, and health state for each ingester strategy.",
        strategy_state.clone(),
    );
    registry.register(
        "strategy_last_persistence_timestamp_seconds",
        "Unix timestamp of the latest durable persistence for each ingester strategy, or zero before first persistence.",
        last_persistence.clone(),
    );
    registry.register(
        "realtime_profiles_desired",
        "Desired running realtime strategy profiles.",
        realtime_profiles_desired.clone(),
    );
    registry.register(
        "realtime_profiles_owned",
        "Desired realtime profiles with a current worker lease.",
        realtime_profiles_owned.clone(),
    );
    registry.register(
        "realtime_profiles_healthy",
        "Desired realtime profiles that are running, healthy, and currently leased.",
        realtime_profiles_healthy.clone(),
    );
    registry.register(
        "worker_realtime_slots_total",
        "Total realtime slots registered by fresh active workers.",
        worker_realtime_slots_total.clone(),
    );
    registry.register(
        "worker_realtime_slots_available",
        "Realtime slots that also have enough free capacity for a standard realtime profile.",
        worker_realtime_slots_available.clone(),
    );
    registry.register(
        "worker_capacity_units_total",
        "Total allocation capacity units registered by fresh active workers.",
        worker_capacity_units_total.clone(),
    );
    registry.register(
        "worker_capacity_units_available",
        "Unallocated capacity units on fresh active workers.",
        worker_capacity_units_available.clone(),
    );
    registry.register(
        "backfill_capacity_units_active",
        "Allocation capacity units currently leased by active backfills.",
        backfill_capacity_units_active.clone(),
    );

    liveness.set(1);
    readiness.set(i64::from(ready));
    let now = Utc::now();
    let desired = profiles
        .iter()
        .filter(|profile| profile.desired_state == crate::domain::DesiredState::Running)
        .collect::<Vec<_>>();
    realtime_profiles_desired.set(i64::try_from(desired.len()).unwrap_or(i64::MAX));
    realtime_profiles_owned.set(
        i64::try_from(
            desired
                .iter()
                .filter(|profile| profile.lease_is_current(now))
                .count(),
        )
        .unwrap_or(i64::MAX),
    );
    realtime_profiles_healthy.set(
        i64::try_from(
            desired
                .iter()
                .filter(|profile| {
                    profile.lease_is_current(now)
                        && profile.observed_state == crate::domain::ObservedState::Running
                        && profile.health_status == crate::domain::HealthStatus::Healthy
                })
                .count(),
        )
        .unwrap_or(i64::MAX),
    );
    for profile in profiles {
        let strategy = profile.strategy_key.as_str().to_owned();
        strategy_state
            .get_or_create(&StrategyStateLabels {
                strategy: strategy.clone(),
                desired_state: profile.desired_state.as_str().to_owned(),
                observed_state: profile.observed_state.as_str().to_owned(),
                health_status: profile.health_status.as_str().to_owned(),
            })
            .set(1);
        last_persistence
            .get_or_create(&StrategyLabels { strategy })
            .set(
                profile
                    .last_persisted_at
                    .map_or(0, |timestamp| timestamp.timestamp()),
            );
    }
    for allocation in allocations {
        let labels = WorkerLabels {
            worker_id: allocation.worker_id.clone(),
        };
        worker_capacity
            .get_or_create(&labels)
            .set(i64::from(allocation.capacity_units));
        worker_allocated
            .get_or_create(&labels)
            .set(allocation.allocated_units);
        worker_realtime
            .get_or_create(&labels)
            .set(allocation.realtime_leases);
        worker_backfills
            .get_or_create(&labels)
            .set(allocation.backfill_leases);
    }
    worker_realtime_slots_total.set(
        allocations
            .iter()
            .map(|allocation| i64::from(allocation.realtime_slot_limit))
            .sum(),
    );
    worker_realtime_slots_available.set(
        allocations
            .iter()
            .filter(|allocation| {
                allocation.realtime_leases < i64::from(allocation.realtime_slot_limit)
                    && allocation.allocated_units + 2 <= i64::from(allocation.capacity_units)
            })
            .count()
            .try_into()
            .unwrap_or(i64::MAX),
    );
    worker_capacity_units_total.set(
        allocations
            .iter()
            .map(|allocation| i64::from(allocation.capacity_units))
            .sum(),
    );
    worker_capacity_units_available.set(
        allocations
            .iter()
            .map(|allocation| {
                (i64::from(allocation.capacity_units) - allocation.allocated_units).max(0)
            })
            .sum(),
    );
    backfill_capacity_units_active.set(
        allocations
            .iter()
            .map(|allocation| allocation.backfill_units)
            .sum(),
    );

    let mut body = String::new();
    encode(&mut body, &registry)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use crate::domain::{DesiredState, HealthStatus, IngesterStrategyKey, ObservedState};

    use super::*;

    #[test]
    fn renders_service_and_strategy_health_without_unbounded_labels() {
        let persisted_at = Utc
            .timestamp_opt(1_787_600_000, 0)
            .single()
            .expect("valid timestamp");
        let profile = IngesterProfile {
            strategy_key: IngesterStrategyKey::PolymarketChainlinkBtcusdTwap,
            config_schema_version: 1,
            config: json!({}),
            desired_state: DesiredState::Running,
            desired_generation: 3,
            observed_state: ObservedState::Degraded,
            health_status: HealthStatus::Degraded,
            applied_generation: Some(3),
            checkpoint_schema_version: 1,
            checkpoint: json!({}),
            lease_owner: None,
            lease_token: None,
            lease_expires_at: None,
            heartbeat_at: None,
            started_at: None,
            stopped_at: None,
            last_source_event_at: None,
            last_provider_available_at: None,
            last_persisted_at: Some(persisted_at),
            source_watermark: None,
            availability_watermark: None,
            consecutive_failures: 0,
            restart_count: 0,
            last_error_code: Some("not_exported_as_a_label".to_owned()),
            last_error_message: Some("also not exported".to_owned()),
            last_error_at: None,
            created_at: persisted_at,
            updated_at: persisted_at,
        };

        let rendered = render(&[profile], &[], true).expect("metrics render");

        assert!(rendered.contains("market_data_ingester_liveness 1"));
        assert!(rendered.contains("market_data_ingester_readiness 1"));
        assert!(
            rendered.contains("market_data_ingester_strategy_state"),
            "{rendered}"
        );
        assert!(rendered.contains("strategy=\"polymarket_chainlink_btcusd_twap\""));
        assert!(rendered.contains("desired_state=\"running\""));
        assert!(rendered.contains("observed_state=\"degraded\""));
        assert!(rendered.contains("health_status=\"degraded\""));
        assert!(rendered.contains(
            "market_data_ingester_strategy_last_persistence_timestamp_seconds{strategy=\"polymarket_chainlink_btcusd_twap\"} 1787600000"
        ));
        assert!(!rendered.contains("not_exported_as_a_label"));
        assert!(!rendered.contains("also not exported"));
    }

    #[test]
    fn renders_exactly_one_bounded_state_and_persistence_series_for_every_strategy() {
        let timestamp = Utc.timestamp_opt(1_787_600_000, 0).single().unwrap();
        let profiles = IngesterStrategyKey::ALL
            .into_iter()
            .map(|strategy_key| IngesterProfile {
                strategy_key,
                config_schema_version: 1,
                config: json!({}),
                desired_state: DesiredState::Stopped,
                desired_generation: 1,
                observed_state: ObservedState::Stopped,
                health_status: HealthStatus::Unknown,
                applied_generation: None,
                checkpoint_schema_version: 1,
                checkpoint: json!({}),
                lease_owner: None,
                lease_token: None,
                lease_expires_at: None,
                heartbeat_at: None,
                started_at: None,
                stopped_at: Some(timestamp),
                last_source_event_at: None,
                last_provider_available_at: None,
                last_persisted_at: None,
                source_watermark: None,
                availability_watermark: None,
                consecutive_failures: 0,
                restart_count: 0,
                last_error_code: None,
                last_error_message: None,
                last_error_at: None,
                created_at: timestamp,
                updated_at: timestamp,
            })
            .collect::<Vec<_>>();

        let rendered = render(&profiles, &[], true).expect("metrics render");
        assert_eq!(
            rendered
                .matches("market_data_ingester_strategy_state{")
                .count(),
            11
        );
        assert_eq!(
            rendered
                .matches("market_data_ingester_strategy_last_persistence_timestamp_seconds{")
                .count(),
            11
        );
        for key in IngesterStrategyKey::ALL {
            assert!(rendered.contains(&format!("strategy=\"{}\"", key.as_str())));
        }
        assert!(!rendered.contains("error_code="));
    }

    #[test]
    fn renders_aggregate_cutover_capacity_metrics() {
        let allocations = vec![
            WorkerAllocationRecord {
                worker_id: "realtime-worker".to_owned(),
                capacity_units: 4,
                realtime_slot_limit: 1,
                allocated_units: 2,
                realtime_leases: 1,
                backfill_leases: 0,
                backfill_units: 0,
            },
            WorkerAllocationRecord {
                worker_id: "backfill-worker".to_owned(),
                capacity_units: 4,
                realtime_slot_limit: 1,
                allocated_units: 3,
                realtime_leases: 0,
                backfill_leases: 1,
                backfill_units: 3,
            },
        ];

        let rendered = render(&[], &allocations, true).expect("metrics render");

        assert!(rendered.contains("market_data_ingester_realtime_profiles_desired 0"));
        assert!(rendered.contains("market_data_ingester_worker_realtime_slots_total 2"));
        assert!(rendered.contains("market_data_ingester_worker_realtime_slots_available 0"));
        assert!(rendered.contains("market_data_ingester_worker_capacity_units_total 8"));
        assert!(rendered.contains("market_data_ingester_worker_capacity_units_available 3"));
        assert!(rendered.contains("market_data_ingester_backfill_capacity_units_active 3"));
    }
}
