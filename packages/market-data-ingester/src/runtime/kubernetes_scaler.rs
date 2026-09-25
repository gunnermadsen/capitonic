//! Reconcile the existing Kubernetes worker Deployment with durable ingester demand.

use std::{
    env, fs,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use reqwest::{Certificate, Client, Method};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::persistence::{BackfillRepository, DrainRepository, ProfileRepository};

const SERVICE_ACCOUNT_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
const RECONCILE_INTERVAL: Duration = Duration::from_secs(10);
const IDLE_HOLD: Duration = Duration::from_secs(60);

pub(crate) struct KubernetesWorkerScaler {
    profiles: ProfileRepository,
    backfills: BackfillRepository,
    drains: DrainRepository,
    client: Client,
    scale_url: String,
    minimum: i32,
    maximum: i32,
}

#[derive(Deserialize)]
struct Scale {
    metadata: ScaleMetadata,
    spec: ScaleSpec,
}

#[derive(Deserialize)]
struct ScaleMetadata {
    #[serde(rename = "resourceVersion")]
    resource_version: String,
}

#[derive(Deserialize)]
struct ScaleSpec {
    replicas: i32,
}

impl KubernetesWorkerScaler {
    pub(crate) fn from_environment(pool: PgPool) -> Result<Option<Self>> {
        match env::var("INGESTER_WORKER_SCALING_MODE").as_deref() {
            Err(env::VarError::NotPresent) | Ok("none") => return Ok(None),
            Ok("kubernetes") => {}
            _ => bail!("INGESTER_WORKER_SCALING_MODE must be none or kubernetes"),
        }
        let minimum = replica_bound("INGESTER_WORKER_MIN_REPLICAS", 1)?;
        let maximum = replica_bound("INGESTER_WORKER_MAX_REPLICAS", 4)?;
        if minimum > maximum {
            bail!("INGESTER_WORKER_MIN_REPLICAS must not exceed INGESTER_WORKER_MAX_REPLICAS");
        }
        let host = env::var("KUBERNETES_SERVICE_HOST")
            .context("KUBERNETES_SERVICE_HOST is required for Kubernetes worker scaling")?;
        let port = env::var("KUBERNETES_SERVICE_PORT")
            .context("KUBERNETES_SERVICE_PORT is required for Kubernetes worker scaling")?
            .parse::<u16>()
            .context("KUBERNETES_SERVICE_PORT must be a TCP port")?;
        let namespace = fs::read_to_string(format!("{SERVICE_ACCOUNT_PATH}/namespace"))
            .context("read Kubernetes service account namespace")?;
        let namespace = namespace.trim();
        if namespace.is_empty()
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            bail!("Kubernetes service account namespace is invalid");
        }
        let ca = fs::read(format!("{SERVICE_ACCOUNT_PATH}/ca.crt"))
            .context("read Kubernetes API CA certificate")?;
        let client = Client::builder()
            .add_root_certificate(
                Certificate::from_pem(&ca).context("parse Kubernetes API CA certificate")?,
            )
            .timeout(Duration::from_secs(5))
            .build()
            .context("build Kubernetes API client")?;
        Ok(Some(Self {
            profiles: ProfileRepository::new(pool.clone()),
            backfills: BackfillRepository::new(pool.clone()),
            drains: DrainRepository::new(pool),
            client,
            scale_url: format!("https://{host}:{port}/apis/apps/v1/namespaces/{namespace}/deployments/ingester-worker/scale"),
            minimum,
            maximum,
        }))
    }

    pub(crate) async fn run(self, shutdown: CancellationToken) -> Result<()> {
        let mut ticker = tokio::time::interval(RECONCILE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut idle_since = None;
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                _ = ticker.tick() => {
                    if let Err(error) = self.reconcile(&mut idle_since).await {
                        idle_since = None;
                        warn!(error = %error, "Kubernetes worker scaling deferred");
                    }
                }
            }
        }
    }

    async fn reconcile(&self, idle_since: &mut Option<Instant>) -> Result<()> {
        let realtime = self.profiles.desired_running_count().await?;
        let backfills = self.backfills.unfinished_shard_count().await?;
        let drains = self.drains.unfinished_count().await?;
        let demand = realtime.saturating_add(backfills).saturating_add(drains);
        let target = desired_replicas(demand, self.minimum, self.maximum);
        let scale = self.get_scale().await?;
        if target > scale.spec.replicas {
            *idle_since = None;
            self.set_scale(&scale, target).await?;
            info!(
                from = scale.spec.replicas,
                to = target,
                realtime,
                backfills,
                drains,
                "requested ingester worker capacity"
            );
        } else if demand == 0 && scale.spec.replicas > self.minimum {
            let allocations = self.backfills.list_worker_allocation_summaries().await?;
            if allocations
                .iter()
                .any(|worker| worker.realtime_leases > 0 || worker.backfill_leases > 0)
            {
                *idle_since = None;
                return Ok(());
            }
            let since = idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= IDLE_HOLD {
                self.set_scale(&scale, self.minimum).await?;
                info!(
                    from = scale.spec.replicas,
                    to = self.minimum,
                    "released idle ingester worker capacity"
                );
                *idle_since = None;
            }
        } else {
            *idle_since = None;
        }
        Ok(())
    }

    async fn get_scale(&self) -> Result<Scale> {
        self.request(Method::GET)
            .await?
            .send()
            .await
            .context("read ingester worker scale")?
            .error_for_status()
            .context("Kubernetes rejected worker scale read")?
            .json()
            .await
            .context("decode ingester worker scale")
    }

    async fn set_scale(&self, current: &Scale, replicas: i32) -> Result<()> {
        self.request(Method::PUT).await?
            .json(&json!({
                "apiVersion": "autoscaling/v1",
                "kind": "Scale",
                "metadata": {"name": "ingester-worker", "resourceVersion": current.metadata.resource_version},
                "spec": {"replicas": replicas}
            }))
            .send().await.context("update ingester worker scale")?
            .error_for_status().context("Kubernetes rejected worker scale update")?;
        Ok(())
    }

    async fn request(&self, method: Method) -> Result<reqwest::RequestBuilder> {
        let token = fs::read_to_string(format!("{SERVICE_ACCOUNT_PATH}/token"))
            .context("read Kubernetes service account token")?;
        Ok(self
            .client
            .request(method, &self.scale_url)
            .bearer_auth(token.trim()))
    }
}

fn replica_bound(name: &str, default: i32) -> Result<i32> {
    let value = env::var(name).map_or(Ok(default), |raw| {
        raw.parse::<i32>()
            .with_context(|| format!("{name} must be an integer"))
    })?;
    if !(1..=16).contains(&value) {
        bail!("{name} must be between 1 and 16");
    }
    Ok(value)
}

fn desired_replicas(demand: i64, minimum: i32, maximum: i32) -> i32 {
    demand.clamp(i64::from(minimum), i64::from(maximum)) as i32
}

#[cfg(test)]
mod tests {
    use super::desired_replicas;

    #[test]
    fn demand_respects_worker_floor_and_ceiling() {
        assert_eq!(desired_replicas(0, 1, 4), 1);
        assert_eq!(desired_replicas(3, 1, 4), 3);
        assert_eq!(desired_replicas(20, 1, 4), 4);
    }
}
