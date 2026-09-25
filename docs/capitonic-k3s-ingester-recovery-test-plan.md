# Local ingester recovery test plan

## Purpose and boundary

Verify that the existing ingester master, `ingester-worker` Deployment, durable profiles and job ledgers recover from ordinary pod lifecycle events while API-scheduled ingestion continues. Run only against the isolated `capitonic` k3s development namespace. No trading process, schema migration, Docker Compose deployment, manual worker pod creation, or source-data removal belongs in this pass.

The maintained runner is `scripts/helm/test-ingester-recovery.py`. It uses the current administrative API for work and Kubernetes only to observe Deployments and replace existing pods. It records a JSON result outside Git. A passing HTTP health check alone is insufficient: observe current owners, advancing heartbeats, shard status, verified coverage, worker registration, and the final one-worker floor.

## Entry and exit gates

- Confirm the worktree is on `feature/capitonic-k3s-ingestion-control`, the intended ingester image is running, the master and standby worker are ready, and the namespace's database and monitoring pods are healthy.
- Confirm zero enabled trading processes, zero desired-running realtime profiles, and zero unfinished backfill or drain jobs. The runner refuses to start otherwise.
- Choose 24 consecutive UTC days of Binance history within its available range. Do not reuse a previously submitted request range; request identity is durable and idempotent.
- After each case, record the relevant job/profile IDs, owners, pod and image identities, start/end timestamps, status transitions, and any master warnings. End with no test profiles or unfinished jobs and one ready worker. On failure, stop test profiles through the API and cancel unfinished test jobs; investigate before another run.

## Cases and acceptance evidence

| ID | Controlled action | Pass evidence | Current result |
| --- | --- | --- | --- |
| R1 | Start two realtime profiles through `/api/ingesters/.../start`; delete one owning worker pod. | Two distinct healthy owners before deletion; the affected profile reacquires a healthy lease on a replacement worker; the other profile stays healthy. | Passed in the 2026-09-25 controlled run. |
| R2 | Delete the master pod while the two profiles run. | Replacement master becomes ready; durable desired generations remain running; both profiles keep or regain healthy leases without another start request. | Passed in the 2026-09-25 controlled run. |
| R3 | Submit six daily backfill shards while those profiles run. | All shards reach completed with verified coverage; both realtime profiles remain healthy; no worker restart loop. Fast completion may use existing capacity without scaling. | Passed in the 2026-09-25 controlled run. |
| R4 | Observe two realtime profiles for a bounded interval, then stop them through the API. | Owners and fresh, advancing heartbeats remain healthy; after leases clear and the idle hold expires, the Deployment reaches one ready worker. A longer run should also prove source timestamps advance and resources stay bounded. | The corrected 90-second heartbeat check passed on 2026-09-25; a multi-hour data-freshness and resource run remains open. |
| R5 | From one idle worker, submit two unrestricted nine-day backfills; observe the Deployment and shard events. | Master requests added replicas while demand remains; new workers register; multiple pods execute shards; all shards complete with verified coverage; replicas return to one after idle. | Earlier six-shard deployment test observed 1→4→1 with assignments across four pods. In the later run, 18 shards completed with 5,184 verified rows before the 10-second reconciliation tick, so no scale-up was observed; a later cancellation/retry workload did trigger 1→4→1. Repeat with a representative longer-running workload for this exact case. |
| R6 | Submit a separate backfill, cancel before all shards finish, then retry through `/api/backfills/{job_id}/retry`. | At least one shard is cancelled, retry requeues it, and all shards complete without manual ledger edits. | Passed in the 2026-09-25 controlled run. |
| R7 | Submit `/api/drains` with `mode=reconcile` and `dry_run=true`. | Existing worker claims and completes the job; `rows_removed=0` and `objects_published=0`; no source-data mutation. | Passed in the 2026-09-25 controlled run. |
| R8 | Interrupt a worker during an **active backfill shard**, then restore capacity. | Expired lease is recovered; work is retried or reassigned exactly under the existing contract; final coverage is complete without a stuck running shard. | Not yet run. Use a workload long enough to observe an active shard before deletion. |
| R9 | Temporarily make Kubernetes `/scale` updates unavailable to the master while queued work exists, then restore access. | Master stays ready and logs bounded retry warnings; jobs persist; scaling resumes automatically after access returns. | Not yet run. A guarded, timed access restoration mechanism is required before injecting this fault. |
| R10 | Run a non-dry-run copy-only drain after durable worker storage is provisioned. | Published Parquet is verified and survives worker replacement; `rows_removed=0`; job recovers from a worker restart. | Blocked by the current worker storage contract; do not run on ephemeral pod storage. |
| R11 | Perform an ingester image rollout during active ingestion using the safe deployment procedure. | Capacity and leases recover; no data gap or stuck job; exact image identity and rollback tuple are recorded. | Not yet run. The chart currently uses `Recreate` for workers, so this deserves separate validation before routine active-work rollouts. |

## Running the automated portion

```sh
python3 scripts/helm/test-ingester-recovery.py --start-date YYYY-MM-DD --soak-seconds 90
```

The full runner executes R1–R7. The targeted modes `--only realtime-soak`, `--only cancel-retry`, and `--only drain-dry-run` rerun those cases without repeating pod replacement. `--soak-seconds` accepts zero to four hours; choose a longer interval for R4 and separately inspect advancing source timestamps and resource trends. Results default to `/private/tmp/capitonic-ingester-recovery.json`; use `--report` to keep separate run records. An `inconclusive_fast_work` scale result means the data work finished before the scaler's polling interval, not that scale-up failed.

## Known limits to observe

- The scaler checks demand every ten seconds and has a one-worker floor and four-worker ceiling. Short jobs may complete before a scale decision. Work beyond four workers' capacity remains queued under the existing scheduler.
- The scaler releases excess replicas only when **all** demand and leases are gone for the idle interval. A fall from four to two active profiles leaves four replicas until the system is fully idle.
- Drain jobs count toward desired replicas, but the drain worker claims from a separate ledger. The dry run verifies claim and completion, not Parquet persistence or mixed drain resource contention.
- Worker and master restart recovery has been observed; active-shard loss, API-access failure, long-duration data freshness, and active-work image rollout need the separate controlled cases above. Do not infer their outcomes from healthy pods alone.
