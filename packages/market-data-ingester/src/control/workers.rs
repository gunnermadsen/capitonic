//! Optional presentation of the existing worker allocation snapshot.

use std::collections::BTreeMap;

use axum::{
    http::header::CONTENT_TYPE,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::persistence::WorkerAllocationSummary;

use super::error::ApiError;

#[derive(Default, Deserialize)]
pub(super) struct WorkersQuery {
    #[serde(default)]
    view: View,
    #[serde(default)]
    format: Format,
}

impl WorkersQuery {
    pub(super) fn is_compact(&self) -> bool {
        matches!(self.view, View::Compact)
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum View {
    #[default]
    Full,
    Compact,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Format {
    #[default]
    Json,
    Yaml,
}

pub(super) fn response(
    workers: Vec<WorkerAllocationSummary>,
    query: WorkersQuery,
) -> Result<Response, ApiError> {
    if query.is_compact() {
        let workers = workers
            .into_iter()
            .map(CompactWorker::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::internal)?;
        render(CompactWorkers { workers }, query.format)
    } else {
        render(workers, query.format)
    }
}

fn render(value: impl Serialize, format: Format) -> Result<Response, ApiError> {
    match format {
        Format::Json => Ok(Json(value).into_response()),
        Format::Yaml => {
            let body = serde_yaml_ng::to_string(&value).map_err(ApiError::internal)?;
            Ok(([(CONTENT_TYPE, "application/yaml")], body).into_response())
        }
    }
}

#[derive(Serialize)]
struct CompactWorkers {
    workers: Vec<CompactWorker>,
}

#[derive(Serialize)]
struct CompactWorker {
    worker_id: String,
    lifecycle_state: String,
    heartbeat_fresh: bool,
    capacity: Capacity,
    realtime: Vec<Realtime>,
    backfills: Vec<Backfill>,
}

#[derive(Serialize)]
struct Capacity {
    total: i32,
    allocated: i64,
}

#[derive(Deserialize, Serialize)]
struct Realtime {
    #[serde(rename(deserialize = "strategy_key"))]
    strategy: String,
    #[serde(rename(deserialize = "observed_state"))]
    state: String,
    #[serde(rename(deserialize = "health_status"))]
    health: String,
}

#[derive(Deserialize)]
struct Assignment {
    job_id: Uuid,
    parent_job_id: Option<Uuid>,
    shard_key: Option<String>,
    strategy_key: String,
    status: String,
}

#[derive(Serialize)]
struct Backfill {
    job_id: Uuid,
    strategy: String,
    shards: Vec<Shard>,
}

#[derive(Serialize)]
struct Shard {
    shard_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    shard_key: Option<String>,
    status: String,
}

impl TryFrom<WorkerAllocationSummary> for CompactWorker {
    type Error = serde_json::Error;

    fn try_from(worker: WorkerAllocationSummary) -> Result<Self, Self::Error> {
        let assignments: Vec<Assignment> = serde_json::from_value(worker.assigned_backfills)?;
        let mut backfills = BTreeMap::new();
        for assignment in assignments {
            let job_id = assignment.parent_job_id.unwrap_or(assignment.job_id);
            let job = backfills.entry(job_id).or_insert_with(|| Backfill {
                job_id,
                strategy: assignment.strategy_key,
                shards: Vec::new(),
            });
            job.shards.push(Shard {
                shard_id: assignment.job_id,
                shard_key: assignment.shard_key,
                status: assignment.status,
            });
        }
        Ok(Self {
            worker_id: worker.worker_id,
            lifecycle_state: worker.lifecycle_state,
            heartbeat_fresh: worker.heartbeat_fresh,
            capacity: Capacity {
                total: worker.capacity_units,
                allocated: worker.allocated_units,
            },
            realtime: serde_json::from_value(worker.assigned_realtime_strategies)?,
            backfills: backfills.into_values().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        extract::Query,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use chrono::{TimeZone, Utc};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    fn worker() -> WorkerAllocationSummary {
        let timestamp = Utc.timestamp_opt(1_788_000_000, 0).single().unwrap();
        WorkerAllocationSummary {
            worker_id: "worker-1".to_owned(),
            hostname: "ingester-worker-1".to_owned(),
            worker_contract_version: 1,
            supported_strategies: json!({"strategy": 1}),
            maximum_backfills: 2,
            active_backfills: 0,
            capacity_units: 4,
            realtime_slot_limit: 1,
            allocation_contract_version: 1,
            realtime_strategies: json!(["binance_spot_btcusdt_l2_snapshots"]),
            image_digest: "sha256:image".to_owned(),
            source_revision: "revision".to_owned(),
            deployment_id: "deployment".to_owned(),
            lifecycle_state: "active".to_owned(),
            started_at: timestamp,
            heartbeat_at: timestamp,
            updated_at: timestamp,
            allocated_units: 2,
            available_units: 2,
            realtime_leases: 1,
            backfill_leases: 0,
            heartbeat_fresh: true,
            assigned_realtime_strategies: json!([{
                "strategy_key": "binance_spot_btcusdt_l2_snapshots",
                "desired_generation": 9,
                "applied_generation": 9,
                "observed_state": "running",
                "health_status": "healthy",
                "lease_expires_at": timestamp
            }]),
            assigned_backfills: json!([]),
        }
    }

    async fn body(response: Response) -> String {
        String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn full_json_remains_unchanged_and_yaml_preserves_values() {
        let expected = serde_json::to_value(vec![worker()]).unwrap();
        for format in [Format::Json, Format::Yaml] {
            let yaml = matches!(format, Format::Yaml);
            let response = response(
                vec![worker()],
                WorkersQuery {
                    view: View::Full,
                    format,
                },
            )
            .unwrap();
            assert_eq!(
                response.headers()[CONTENT_TYPE],
                if yaml {
                    "application/yaml"
                } else {
                    "application/json"
                }
            );
            let text = body(response).await;
            let value: Value = if yaml {
                serde_yaml_ng::from_str(&text).unwrap()
            } else {
                serde_json::from_str(&text).unwrap()
            };
            assert_eq!(value, expected);
        }
    }

    #[tokio::test]
    async fn compact_groups_shards_by_parent_and_keeps_idle_workers() {
        let mut busy = worker();
        busy.assigned_backfills = json!([
            {"job_id": Uuid::from_u128(3), "parent_job_id": Uuid::from_u128(1), "shard_key": "2026-10-01", "strategy_key": "history", "status": "running"},
            {"job_id": Uuid::from_u128(4), "parent_job_id": Uuid::from_u128(2), "shard_key": "2026-10-02", "strategy_key": "other-history", "status": "cancel_requested"},
            {"job_id": Uuid::from_u128(5), "parent_job_id": Uuid::from_u128(1), "shard_key": "2026-10-03", "strategy_key": "history", "status": "running"}
        ]);
        let mut idle = worker();
        idle.worker_id = "worker-idle".into();
        idle.heartbeat_fresh = false;
        idle.assigned_realtime_strategies = json!([]);
        let input = vec![busy, idle];
        let json_response = response(
            input.clone(),
            WorkersQuery {
                view: View::Compact,
                format: Format::Json,
            },
        )
        .unwrap();
        let value: Value = serde_json::from_str(&body(json_response).await).unwrap();
        assert_eq!(
            value["workers"][0]["backfills"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            value["workers"][0]["backfills"][0]["job_id"],
            Uuid::from_u128(1).to_string()
        );
        assert_eq!(
            value["workers"][0]["backfills"][0]["shards"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            value["workers"][0]["backfills"][0]["shards"][1]["shard_id"],
            Uuid::from_u128(5).to_string()
        );
        assert_eq!(
            value["workers"][0]["backfills"][1]["shards"][0]["status"],
            "cancel_requested"
        );
        assert_eq!(value["workers"][0]["realtime"][0]["state"], "running");
        assert!(value["workers"][0].get("supported_strategies").is_none());
        assert_eq!(value["workers"][1]["backfills"], json!([]));
        assert_eq!(value["workers"][1]["realtime"], json!([]));
        assert_eq!(value["workers"][1]["heartbeat_fresh"], false);
        let yaml_response = response(
            input,
            WorkersQuery {
                view: View::Compact,
                format: Format::Yaml,
            },
        )
        .unwrap();
        let yaml = body(yaml_response).await;
        assert!(yaml.starts_with("workers:\n"));
        assert!(yaml.contains("  backfills:\n"));
        assert_eq!(serde_yaml_ng::from_str::<Value>(&yaml).unwrap(), value);
    }

    #[tokio::test]
    async fn query_defaults_and_invalid_values() {
        let router = Router::new().route(
            "/workers",
            get(|Query(query): Query<WorkersQuery>| async { response(vec![], query) }),
        );
        for (uri, status, expected) in [
            ("/workers", StatusCode::OK, Some("[]")),
            (
                "/workers?view=compact",
                StatusCode::OK,
                Some("{\"workers\":[]}"),
            ),
            (
                "/workers?view=compact&format=yaml",
                StatusCode::OK,
                Some("workers: []\n"),
            ),
            ("/workers?format=yaml", StatusCode::OK, Some("[]\n")),
            ("/workers?format=xml", StatusCode::BAD_REQUEST, None),
            ("/workers?view=unknown", StatusCode::BAD_REQUEST, None),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{uri}");
            if let Some(expected) = expected {
                assert_eq!(body(response).await, expected, "{uri}");
            }
        }
    }
}
