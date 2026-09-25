#!/usr/bin/env python3
"""Exercise local k3s ingester recovery through its existing API and Deployment."""

import argparse
import datetime as dt
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
NAMESPACE = "capitonic"
BACKFILL = "binance_futures_btcusdt_five_minute_open_interest_backfill"
PROFILES = (
    "binance_futures_btcusdt_open_interest",
    "polymarket_btc_five_minute_market_contracts",
)


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def command(*args):
    result = subprocess.run(args, text=True, capture_output=True, timeout=30, check=False)
    if result.returncode:
        raise RuntimeError(f"{' '.join(args)}: {result.stderr.strip()}")
    return result.stdout.strip()


def kubectl(*args):
    return command("kubectl", "-n", NAMESPACE, *args)


def wait_for(label, predicate, seconds=120, interval=2):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            last = predicate()
            if last:
                return last
        except Exception as error:
            last = str(error)
        time.sleep(interval)
    raise AssertionError(f"timed out waiting for {label}; last={last!r}")


def env_value(path, key):
    for line in path.read_text().splitlines():
        if line.startswith(key + "="):
            return line.partition("=")[2].strip().strip('"').strip("'")
    raise RuntimeError(f"{key} is absent from {path.name}")


class Suite:
    def __init__(self, start_date, soak_seconds):
        self.token = env_value(ROOT / ".env.market-data-ingester", "MARKET_DATA_INGESTER_ADMIN_TOKEN")
        self.start_date = start_date
        self.soak_seconds = soak_seconds
        self.started_profiles = set()
        self.created_jobs = set()
        self.results = []

    def api(self, method, path, body=None, headers=None):
        request_headers = {"Authorization": "Bearer " + self.token}
        request_headers.update(headers or {})
        data = (b"" if method == "POST" else None) if body is None else json.dumps(body).encode()
        if data is not None:
            request_headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            "http://localhost/api" + path, data=data, headers=request_headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=15) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise RuntimeError(f"{method} {path}: HTTP {error.code}: {error.read().decode()[:300]}") from error

    def sql_count(self, query):
        return int(kubectl("exec", "timescaledb-0", "-c", "timescaledb", "--",
                           "psql", "-U", "postgres", "-d", "polymarket", "-Atqc", query))

    def deployment(self):
        return json.loads(kubectl("get", "deployment", "ingester-worker", "-o", "json"))

    def worker_replicas(self):
        deployment = self.deployment()
        return deployment["spec"]["replicas"], deployment["status"].get("readyReplicas", 0)

    def pods(self, name):
        items = json.loads(kubectl("get", "pods", "-l", f"app.kubernetes.io/name={name}", "-o", "json"))["items"]
        return [pod for pod in items if pod["status"]["phase"] == "Running"]

    def profile(self, key):
        return self.api("GET", "/ingesters/" + key)

    def set_profile(self, key, action):
        current = self.profile(key)
        result = self.api("POST", f"/ingesters/{key}/{action}", headers={"If-Match": str(current["desired_generation"])})
        if action == "start":
            self.started_profiles.add(key)
        else:
            self.started_profiles.discard(key)
        return result

    def backfill(self, start, days):
        end = start + dt.timedelta(days=days)
        job = self.api("POST", "/backfills", {
            "strategy_key": BACKFILL,
            "range": {"start": start.isoformat() + "T00:00:00Z", "end": end.isoformat() + "T00:00:00Z"},
            "parameters": {}, "execution": {},
        })
        check(job["status"] == "queued", "backfill request was reused; choose an unused start date")
        self.created_jobs.add(job["job_id"])
        return job["job_id"]

    def job(self, job_id):
        return self.api("GET", "/backfills/" + job_id)

    def preflight(self):
        check(command("git", "branch", "--show-current") == "feature/capitonic-k3s-ingestion-control",
              "run from the ingestion-control worktree branch")
        check(self.sql_count("SELECT count(*) FROM polymarket.trading_processes WHERE enabled IS TRUE") == 0,
              "an enabled trading process exists")
        check(self.sql_count("SELECT count(*) FROM ingester.profiles WHERE desired_state='running'") == 0,
              "a realtime profile is already running")
        check(self.sql_count("SELECT count(*) FROM ingester.backfill_jobs WHERE job_kind='shard' AND status IN ('queued','running','cancel_requested')") == 0,
              "unfinished backfill shards exist")
        check(self.sql_count("SELECT count(*) FROM ingester.drain_jobs WHERE status IN ('queued','running')") == 0,
              "unfinished drain jobs exist")
        check(self.worker_replicas() == (1, 1), "worker Deployment is not at its healthy idle floor")
        check(urllib.request.urlopen("http://localhost/api/health/ready", timeout=5).status == 200,
              "master ingress is not ready")
        self.results.append({"case": "preflight", "result": "pass"})
        print("PASS preflight", flush=True)

    def realtime_recovery(self):
        for key in PROFILES:
            self.set_profile(key, "start")
        def healthy():
            profiles = [self.profile(key) for key in PROFILES]
            owners = {p["lease_owner"] for p in profiles}
            return profiles if all(p["observed_state"] == "running" and p["health_status"] == "healthy"
                                   and p["lease_owner"] for p in profiles) and len(owners) == 2 else None
        before = wait_for("two healthy realtime owners", healthy, 120)
        wait_for("two ready workers", lambda: self.worker_replicas() == (2, 2), 120)
        old_owner = before[0]["lease_owner"]
        kubectl("delete", "pod", old_owner, "--wait=false")
        after_worker_loss = wait_for(
            "realtime lease reassignment", lambda: (p if (p := self.profile(PROFILES[0]))["lease_owner"]
            not in (None, old_owner) and p["health_status"] == "healthy" else None), 180)
        old_master = self.pods("ingester-master")[0]["metadata"]["name"]
        kubectl("delete", "pod", old_master, "--wait=false")
        wait_for("new ready master", lambda: self.pods("ingester-master")[0]["metadata"]["name"] != old_master
                 and self.api("GET", "/ingesters/" + PROFILES[0]), 180)
        final = wait_for("healthy realtime after master restart", healthy, 120)
        self.results.append({"case": "realtime_recovery", "result": "pass", "old_owner": old_owner,
                             "new_owner": after_worker_loss["lease_owner"],
                             "owners_after_master_restart": [p["lease_owner"] for p in final]})
        print("PASS realtime recovery", flush=True)

    def mixed_workload(self):
        job_id = self.backfill(self.start_date + dt.timedelta(days=6), 6)
        finished = wait_for("mixed-workload shards", lambda: (j if (j := self.job(job_id))["job"]["status"]
                            == "completed" else None), 180)
        check(len(finished["shards"]) == 6 and all(s["status"] == "completed" for s in finished["shards"]),
              "mixed-workload shards did not complete")
        check(all(self.profile(key)["health_status"] == "healthy" for key in PROFILES),
              "realtime profile became unhealthy during backfill")
        self.results.append({"case": "mixed_workload", "result": "pass", "job_id": job_id,
                             "shards": 6, "worker_replicas": self.worker_replicas()[0]})
        print("PASS mixed workload", flush=True)

    def backfill_scaling(self):
        job_ids = (self.backfill(self.start_date, 9),
                   self.backfill(self.start_date + dt.timedelta(days=9), 9))
        peak = 1
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline:
            peak = max(peak, self.worker_replicas()[0])
            if all(self.job(job_id)["job"]["status"] == "completed" for job_id in job_ids):
                break
            time.sleep(1)
        else:
            raise AssertionError("backfill requests did not complete within four minutes")
        finished = [wait_for("completed backfill", lambda job_id=job_id: (j if (j := self.job(job_id))["job"]["status"]
                             == "completed" else None), 240) for job_id in job_ids]
        shards = [shard for job in finished for shard in job["shards"]]
        check(len(shards) == 18 and all(s["status"] == "completed" for s in shards),
              "some backfill shards did not complete")
        owners = set()
        for shard in shards:
            events = self.api("GET", "/backfills/" + shard["job_id"] + "/events")
            owners.update(e["metadata"]["worker_id"] for e in events if e["event_code"] == "job_assigned")
        rows = sum(s["verified_coverage"].get("records_verified", 0) for s in shards)
        check(rows > 0, "backfill returned no verified data")
        self.results.append({"case": "backfill_scaling", "result": "pass" if peak >= 2 and len(owners) >= 2 else "inconclusive_fast_work", "job_ids": job_ids,
                             "workers": sorted(owners), "shards": 18, "verified_rows": rows,
                             "peak_replicas": peak})
        print("PASS backfill completion; peak replicas:", peak, flush=True)

    def cancel_retry(self):
        job_id = self.backfill(self.start_date + dt.timedelta(days=18), 6)
        self.api("POST", "/backfills/" + job_id + "/cancel")
        cancelled = wait_for("cancelled backfill", lambda: (j if (j := self.job(job_id))["job"]["status"]
                             in ("cancelled", "completed") else None), 120)
        cancelled_count = sum(s["status"] == "cancelled" for s in cancelled["shards"])
        check(cancelled_count > 0,
              "cancellation did not reach any shard")
        self.api("POST", "/backfills/" + job_id + "/retry")
        finished = wait_for("retried backfill completion", lambda: (j if (j := self.job(job_id))["job"]["status"]
                            == "completed" else None), 180)
        check(len(finished["shards"]) == 6 and all(s["status"] == "completed" for s in finished["shards"]),
              "retry did not complete all shards")
        self.results.append({"case": "cancel_retry", "result": "pass", "job_id": job_id,
                             "cancelled_shards": cancelled_count})
        print("PASS cancellation and retry", flush=True)

    def drain_dry_run(self):
        cutoff = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=15)).isoformat().replace("+00:00", "Z")
        job = self.api("POST", "/drains", {"strategy_key": "binance_futures_btcusdt_open_interest",
                   "cutoff": cutoff, "mode": "reconcile", "dry_run": True, "execution": {}})
        job_id = job["job_id"]
        finished = wait_for("read-only drain completion", lambda: (j if (j := self.api("GET", "/drains/" + job_id))["status"]
                            in ("completed", "failed", "cancelled") else None), 120)
        check(finished["status"] == "completed", f"dry-run drain failed: {finished.get('last_error_code')}")
        check(finished["rows_removed"] == 0 and finished["objects_published"] == 0,
              "dry-run drain changed source or published output")
        self.results.append({"case": "drain_dry_run", "result": "pass", "job_id": job_id,
                             "summary": finished["summary"]})
        print("PASS drain dry run", flush=True)

    def soak(self):
        if not self.soak_seconds:
            return
        deadline = time.monotonic() + self.soak_seconds
        checks = 0
        previous_heartbeats = {}
        while time.monotonic() < deadline:
            for key in PROFILES:
                profile = self.profile(key)
                check(profile["observed_state"] == "running" and profile["health_status"] == "healthy"
                      and profile["lease_owner"] and profile["heartbeat_at"],
                      f"realtime profile became unhealthy: {key}")
                heartbeat = dt.datetime.fromisoformat(profile["heartbeat_at"].replace("Z", "+00:00"))
                check((dt.datetime.now(dt.timezone.utc) - heartbeat).total_seconds() < 30,
                      f"realtime heartbeat is stale: {key}")
                if key in previous_heartbeats:
                    check(heartbeat > previous_heartbeats[key], f"realtime heartbeat did not advance: {key}")
                previous_heartbeats[key] = heartbeat
            check(self.worker_replicas()[1] >= 2, "worker capacity fell below realtime demand")
            checks += 1
            time.sleep(min(30, max(0, deadline - time.monotonic())))
        self.results.append({"case": "soak", "result": "pass", "seconds": self.soak_seconds, "checks": checks})
        print(f"PASS {self.soak_seconds}-second soak", flush=True)

    def cleanup(self):
        errors = []
        for job_id in self.created_jobs:
            try:
                job = self.job(job_id)["job"]
                if job["status"] in ("queued", "running", "cancel_requested"):
                    self.api("POST", "/backfills/" + job_id + "/cancel")
            except Exception as error:
                errors.append(f"cancel {job_id}: {error}")
        for key in tuple(self.started_profiles):
            try:
                self.set_profile(key, "stop")
            except Exception as error:
                errors.append(f"stop {key}: {error}")
        if not errors and (self.created_jobs or self.started_profiles or self.results):
            try:
                wait_for("idle worker floor", lambda: self.worker_replicas() == (1, 1), 150)
            except Exception as error:
                errors.append(str(error))
        return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--start-date", type=dt.date.fromisoformat, required=True,
                        help="unused UTC start date for 24 Binance history days, within provider retention")
    parser.add_argument("--soak-seconds", type=int, default=0)
    parser.add_argument("--only", choices=("all", "realtime-soak", "cancel-retry", "drain-dry-run"), default="all")
    parser.add_argument("--report", type=Path, default=Path("/private/tmp/capitonic-ingester-recovery.json"))
    args = parser.parse_args()
    check(0 <= args.soak_seconds <= 14400, "soak must be between zero and four hours")
    check(args.start_date + dt.timedelta(days=24) < dt.datetime.now(dt.timezone.utc).date(),
          "test history must end before today")
    suite = Suite(args.start_date, args.soak_seconds)
    error = None
    try:
        suite.preflight()
        if args.only == "all":
            suite.realtime_recovery()
            suite.mixed_workload()
            suite.soak()
            for key in PROFILES:
                suite.set_profile(key, "stop")
            wait_for("idle worker floor before backfill scale", lambda: suite.worker_replicas() == (1, 1), 150)
            suite.backfill_scaling()
        if args.only == "realtime-soak":
            check(args.soak_seconds > 0, "realtime-soak requires --soak-seconds")
            for key in PROFILES:
                suite.set_profile(key, "start")
            wait_for("two healthy realtime profiles", lambda: all(
                suite.profile(key)["health_status"] == "healthy" and suite.profile(key)["lease_owner"]
                for key in PROFILES), 120)
            wait_for("two ready realtime workers", lambda: suite.worker_replicas() == (2, 2), 120)
            suite.soak()
        if args.only in ("all", "cancel-retry"):
            suite.cancel_retry()
        if args.only in ("all", "drain-dry-run"):
            suite.drain_dry_run()
    except Exception as failure:
        error = str(failure)
        print("FAIL", error, file=sys.stderr, flush=True)
    finally:
        cleanup_errors = suite.cleanup()
        report = {"branch": "feature/capitonic-k3s-ingestion-control",
                  "recorded_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                  "cases": suite.results, "error": error, "cleanup_errors": cleanup_errors}
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        print(f"report: {args.report}", flush=True)
        for item in cleanup_errors:
            print("CLEANUP ERROR", item, file=sys.stderr, flush=True)
    return 1 if error or cleanup_errors else 0


if __name__ == "__main__":
    sys.exit(main())
