"""Offline source/arm coverage figures from existing tournament audit tables only."""

from __future__ import annotations

import argparse
import hashlib
import json
import resource
import sys
from collections import Counter, defaultdict
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import plotly
import plotly.graph_objects as go
import polars as pl

MANDATORY = (
    "chainlink_btcusd_reference_prices",
    "pmdata_chainlink_btcusd_reference_prices",
    "polymarket_chainlink_btcusd_twap",
    "pmdata_chainlink_btcusd_twap",
)
CORE = ("rtds_reference_price_ticks", "polymarket_clob_l2")
SOURCE_LABELS = [
    "No hourly record / unknown",
    "Raw observed; no eligible evidence",
    "Partial eligible evidence",
    "Observed eligible evidence",
]
ARM_LABELS = [
    "No coverage record / unknown",
    "Pending causal reference",
    "Measured: zero complete rows",
    "Measured: partial complete rows",
    "Measured: all rows complete",
]
COLORS = ["#d8dde5", "#aa81ba", "#e6b65c", "#258882"]
ARM_COLORS = ["#d8dde5", "#aa81ba", "#f0d7cf", "#e6b65c", "#258882"]
LIMITATIONS = [
    "Source hours use the audit's native source/event coordinate, not receipt-hour publication.",
    (
        "Eligible means existing source validation accepted identity/value/availability evidence. "
        "It does not establish contemporaneous availability, decision freshness, or uninterrupted capture."
    ),
    (
        "Partial source evidence means some raw rows were excluded or fewer than 24 source hours "
        "have eligible rows in a day; duplicate removal can also reduce eligible/raw ratios."
    ),
    "Missing records do not prove no archive exists elsewhere, zero activity, or causality.",
    (
        "Core RTDS hourly counts combine direct_binance and rtds_chainlink because the existing "
        "hourly audit does not split them. No source reread is performed to invent that split."
    ),
    (
        "TWAP raw hourly counts combine the preserved 30/60-second windows. Arm coverage enforces "
        "its own existing complete-case definition; hourly counts alone do not prove both windows."
    ),
    (
        "Arm calendars use coverage counts only, including calibration/evaluation/holdout roles. "
        "No outcomes, predictions, economics, or model rankings are read."
    ),
    "Pending quiet-reference eligibility stays unknown, never measured zero.",
    (
        "Kaleido is not required: figures are offline Plotly HTML with a local shared plotting "
        "asset and SVG export available in the toolbar. No dependency or service is installed."
    ),
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def utc(value: str | datetime) -> datetime:
    value = datetime.fromisoformat(value) if isinstance(value, str) else value
    if value.tzinfo is None:
        raise ValueError("Source hours must have a UTC offset")
    return value.astimezone(UTC)


def source_status(raw: int | None, eligible: int | None) -> int:
    """Classify evidence, never infer capture continuity from a row count."""
    if raw is None:
        if eligible is not None:
            raise ValueError("Eligibility without a raw coverage record")
        return 0
    if raw < 0 or (eligible is not None and not 0 <= eligible <= raw):
        raise ValueError("Invalid source coverage counts")
    if not raw or not eligible:
        return 1
    return 2 if eligible < raw else 3


def source_calendar(
    records: list[dict], product: str, days: list[date]
) -> tuple[list[list[int]], list[list[str]], list[int], list[dict]]:
    by_hour = {}
    for row in records:
        if row["product"] != product:
            continue
        hour = utc(row["hour"])
        if hour.minute or hour.second or hour.microsecond or hour in by_hour:
            raise ValueError("Duplicate or unaligned source hour")
        source_status(row["raw_rows"], row["eligible_rows"])
        by_hour[hour] = row
    states = [[0 for _ in days] for _ in range(24)]
    details = [["" for _ in days] for _ in range(24)]
    daily, summaries = [], []
    for j, day in enumerate(days):
        eligible_hours, raw_hours, partial_hours, raw_total, good_total = 0, 0, 0, 0, 0
        for h in range(24):
            hour = datetime.combine(day, datetime.min.time(), tzinfo=UTC) + timedelta(hours=h)
            row = by_hour.get(hour)
            raw = row["raw_rows"] if row else None
            eligible = row["eligible_rows"] if row else None
            status = source_status(raw, eligible)
            states[h][j] = status
            details[h][j] = (
                f"{product}<br>{day} {h:02d}:00 UTC<br>{SOURCE_LABELS[status]}"
                f"<br>Raw rows: {raw if raw is not None else 'unknown'}"
                f"<br>Eligible rows: {eligible if eligible is not None else 'not recorded'}"
            )
            raw_hours += row is not None
            eligible_hours += bool(eligible)
            partial_hours += status == 2
            raw_total += raw or 0
            good_total += eligible or 0
        status = 0 if not raw_hours else 1 if not eligible_hours else 2
        if eligible_hours == 24 and not partial_hours:
            status = 3
        daily.append(status)
        summaries.append(
            {
                "date": str(day),
                "raw_hours": raw_hours,
                "eligible_hours": eligible_hours,
                "raw_rows": raw_total,
                "eligible_rows": good_total,
                "status": status,
            }
        )
    return states, details, daily, summaries


def arm_calendar(rows: list[dict], days: list[date]) -> tuple[list[str], list, list]:
    grouped = defaultdict(list)
    for row in rows:
        grouped[(row["candidate"], row["arm"], row["date"])].append(row)
    arms = sorted({(row["candidate"], row["arm"]) for row in rows})
    states, details = [], []
    for candidate, arm in arms:
        values, texts = [], []
        for day in days:
            cell = grouped.get((candidate, arm, str(day)), [])
            state, denominator, numerator = 0, None, None
            if cell:
                statuses = {r["status"] for r in cell}
                if statuses == {"pending_causal_simulated_refresh"}:
                    if any(r["eligible_rows"] is not None for r in cell):
                        raise ValueError("Pending eligibility cannot be measured zero")
                    state = 1
                    denominator = sum(r["decision_rows"] for r in cell)
                elif statuses == {"measured"}:
                    if any(r["eligible_rows"] is None for r in cell):
                        raise ValueError("Measured eligibility cannot be null")
                    denominator = sum(r["decision_rows"] for r in cell)
                    numerator = sum(r["eligible_rows"] for r in cell)
                    if not 0 <= numerator <= denominator:
                        raise ValueError("Invalid arm eligibility counts")
                    state = 2 if not numerator else 4 if numerator == denominator else 3
                else:
                    raise ValueError("Mixed or unsupported coverage status")
            fraction = (
                f"{numerator / denominator:.2%}"
                if denominator and numerator is not None
                else "unknown"
            )
            values.append(state)
            texts.append(
                f"{candidate} / {arm}<br>{day}<br>{ARM_LABELS[state]}"
                f"<br>Complete / decisions: {numerator} / {denominator} ({fraction})"
                f"<br>Roles: {', '.join(sorted({r['role'] for r in cell})) or 'unknown'}"
            )
        states.append(values)
        details.append(texts)
    return [f"{candidate} / {arm}" for candidate, arm in arms], states, details


def split_markers(figure: go.Figure, frozen: dict) -> None:
    for index, fold in enumerate(frozen["folds"]):
        color = "#3255a5" if fold["name"] == "holdout" else "#594372"
        for field in ("calibration_start", "evaluation_start", "evaluation_end"):
            figure.add_shape(
                type="line",
                x0=fold[field],
                x1=fold[field],
                y0=0,
                y1=1,
                xref="x",
                yref="paper",
                line={
                    "color": color,
                    "width": 1.2 if field == "evaluation_start" else 0.7,
                    "dash": "solid" if field == "evaluation_start" else "dot",
                },
            )
        figure.add_annotation(
            x=fold["evaluation_start"],
            y=1.04 + (index % 2) * 0.045,
            xref="x",
            yref="paper",
            text=fold["name"],
            showarrow=False,
            xanchor="left",
            font={"size": 10, "color": color},
        )


def figure(
    title: str,
    subtitle: str,
    days: list[date],
    labels: list,
    states: list,
    details: list,
    frozen: dict,
    *,
    arms: bool = False,
) -> go.Figure:
    colors, names = (ARM_COLORS, ARM_LABELS) if arms else (COLORS, SOURCE_LABELS)
    count = len(colors)
    scale = [
        [edge, color] for i, color in enumerate(colors) for edge in (i / count, (i + 1) / count)
    ]
    fig = go.Figure(
        go.Heatmap(
            x=[str(day) for day in days],
            y=labels,
            z=states,
            zmin=-0.5,
            zmax=count - 0.5,
            colorscale=scale,
            customdata=details,
            hovertemplate="%{customdata}<extra></extra>",
            xgap=0,
            ygap=1,
            colorbar={
                "tickvals": list(range(count)),
                "ticktext": names,
                "len": 0.65,
                "thickness": 13,
                "x": 1.015,
                "tickfont": {"size": 11},
            },
        )
    )
    fig.update_layout(
        title={"text": title + "<br><sup>" + subtitle + "</sup>", "x": 0.015, "font": {"size": 20}},
        template="plotly_white",
        width=1700,
        height=max(620, len(labels) * 23 + 210),
        margin={
            "l": 430 if arms else 340 if len(labels) != 24 else 85,
            "r": 290,
            "t": 145,
            "b": 95,
        },
        font={"family": "Arial, sans-serif", "size": 11},
        paper_bgcolor="white",
    )
    fig.update_xaxes(
        type="date",
        title="UTC date · full observed source calendar",
        dtick="M1",
        tickformat="%b %Y",
        range=[str(days[0]), str(days[-1] + timedelta(days=1))],
    )
    fig.update_yaxes(autorange="reversed", title="UTC hour" if len(labels) == 24 else None)
    split_markers(fig, frozen)
    fig.add_annotation(
        x=0,
        y=-0.18,
        xref="paper",
        yref="paper",
        xanchor="left",
        showarrow=False,
        text="Solid lines: frozen evaluation starts. Dotted: calibration starts / evaluation ends. "
        "Missing ≠ zero activity. Source eligibility ≠ continuous capture or decision freshness.",
        font={"size": 10, "color": "#495266"},
    )
    return fig


def build(run: Path) -> dict:
    if not run.is_dir() or not str(run.resolve()).startswith("/Volumes/docker-data/"):
        raise ValueError("Existing canonical SSD run required")
    if pl.thread_pool_size() > 2:
        raise ValueError("Set POLARS_MAX_THREADS=2 before running coverage charts")
    inputs = []

    def read_json(relative: str) -> dict:
        path = run / relative
        data = json.loads(path.read_text())
        inputs.append({"path": str(path), "sha256": sha256(path), "kind": "manifest"})
        return data

    def read_table(path: Path, expected: dict | None = None) -> pl.DataFrame:
        if not path.resolve().is_relative_to((run / "metrics").resolve()):
            raise ValueError("Only existing metrics tables may be read")
        digest = sha256(path)
        if expected and digest != expected["sha256"]:
            raise ValueError(f"Coverage input hash mismatch: {path}")
        frame = pl.read_parquet(path)
        if expected and frame.height != expected["rows"]:
            raise ValueError(f"Coverage input count mismatch: {path}")
        inputs.append(
            {"path": str(path), "sha256": digest, "rows": frame.height, "kind": "coverage_table"}
        )
        return frame

    frozen = read_json("inputs/tournament-freeze.json")
    read_json("manifests/optional-source-causal-contract.json")
    records = []
    for product in MANDATORY:
        audit = read_json(f"manifests/{product}-audit.json")
        frame = read_table(run / "metrics" / f"{product}-hourly-coverage.parquet")
        if frame["raw_rows"].sum() != sum(d["raw_rows"] for d in audit["daily"]):
            raise ValueError("Mandatory hourly raw counts disagree with audit")
        if frame["eligible_rows"].sum() != sum(d["causally_eligible_rows"] for d in audit["daily"]):
            raise ValueError("Mandatory hourly eligible counts disagree with audit")
        records.extend(frame.to_dicts())
    for product in CORE:
        audit = read_json(f"manifests/core-source-validation-{product}.json")
        if audit["status"] != "complete":
            raise ValueError("Core coverage audit is incomplete")
        records.extend(
            dict(row, product=product)
            for day in audit["days"].values()
            for row in day["hour_counts"]
        )
    flow = read_table(run / "metrics/flow-source-hourly-coverage.parquet")
    for product in sorted(flow["product"].unique()):
        audit = read_json(f"manifests/flow-source-validation-{product}.json")
        if audit["status"] != "completed":
            raise ValueError("Flow coverage audit is incomplete")
        expected = [row for day in audit["daily"] for row in day["coverage"]]
        actual = flow.filter(pl.col("product") == product)
        for column in ("raw_rows", "eligible_rows"):
            if actual[column].sum() != sum(row[column] or 0 for row in expected):
                raise ValueError("Flow hourly counts disagree with validation manifest")
    records.extend(flow.to_dicts())
    exploration = read_json("manifests/training-exploration.json")
    arm_rows = []
    for day in exploration["days"]:
        item = day["artifacts"]["arm-coverage"]
        frame = read_table(Path(item["path"]), item)
        arm_rows.extend(frame.to_dicts())
    first = min(
        date.fromisoformat(frozen["range_start"]), min(utc(r["hour"]).date() for r in records)
    )
    end = max(
        date.fromisoformat(frozen["range_end_exclusive"]),
        max(utc(r["hour"]).date() for r in records) + timedelta(days=1),
    )
    days = [first + timedelta(days=i) for i in range((end - first).days)]
    out = run / "diagnostics/source-coverage"
    identity = {"inputs": inputs, "implementation_sha256": sha256(Path(__file__))}
    manifest_path = out / "manifest.json"
    if out.exists() and any(out.iterdir()):
        if not manifest_path.exists():
            raise ValueError("Preserve existing incomplete coverage directory")
        saved = json.loads(manifest_path.read_text())
        if saved["identity"] != identity or any(
            sha256(Path(p["path"])) != p["sha256"] for p in saved["outputs"]
        ):
            raise ValueError("Existing coverage outputs do not match this input identity")
        return saved
    out.mkdir(parents=True, exist_ok=True)
    outputs, product_summaries, overview, overview_text = [], {}, [], []

    def publish(name: str, fig: go.Figure) -> None:
        path = out / f"{name}.html"
        fig.write_html(
            path,
            include_plotlyjs="directory",
            div_id=name,
            config={"displaylogo": False, "toImageButtonOptions": {"format": "svg"}},
        )
        outputs.append({"path": str(path), "sha256": sha256(path), "type": "offline_plotly_figure"})
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if (peak if sys.platform == "darwin" else peak * 1024) > 512 * 1024**2:
            raise RuntimeError("Coverage chart process exceeded 512 MiB budget")

    products = sorted({row["product"] for row in records})
    for product in products:
        states, details, daily, summary = source_calendar(records, product, days)
        overview.append(daily)
        overview_text.append(
            [
                f"{product}<br>{s['date']}<br>{SOURCE_LABELS[s['status']]}"
                f"<br>Hours with eligible rows: {s['eligible_hours']}/24"
                f"<br>Hours with raw records: {s['raw_hours']}/24"
                f"<br>Raw / eligible rows: {s['raw_rows']} / {s['eligible_rows']}"
                for s in summary
            ]
        )
        product_summaries[product] = {
            "hourly_status_counts": dict(Counter(value for row in states for value in row)),
            "daily_status_counts": dict(Counter(daily)),
        }
        publish(
            product,
            figure(
                product + " · source evidence by UTC hour",
                "Existing audit counts only · event/source hours · hover for raw and eligible counts",
                days,
                list(range(24)),
                states,
                details,
                frozen,
            ),
        )
    publish(
        "source-calendar",
        figure(
            "BTC tournament · source evidence calendar",
            "Full source history · daily states summarize all 24 UTC hours · counts do not prove continuity",
            days,
            products,
            overview,
            overview_text,
            frozen,
        ),
    )
    labels, states, details = arm_calendar(arm_rows, days)
    publish(
        "arm-calendar",
        figure(
            "BTC tournament · every declared arm's complete-case coverage",
            "Coverage only across all frozen roles · pending eligibility remains unknown · hover for counts",
            days,
            labels,
            states,
            details,
            frozen,
            arms=True,
        ),
    )
    asset = out / "plotly.min.js"
    outputs.append({"path": str(asset), "sha256": sha256(asset), "type": "local_plotting_asset"})
    report = {
        "schema_version": "btc-source-coverage-charts-v1",
        "status": "complete",
        "created_at": datetime.now(UTC).isoformat(),
        "identity": identity,
        "calendar_start": str(first),
        "calendar_end_exclusive": str(end),
        "days": len(days),
        "source_products": len(products),
        "arms": len(labels),
        "source_summaries": product_summaries,
        "source_status_labels": SOURCE_LABELS,
        "arm_status_labels": ARM_LABELS,
        "boundaries": frozen["folds"],
        "plotly_version": plotly.__version__,
        "outputs": outputs,
        "limitations": LIMITATIONS,
        "visual_inspection": "pending separate local browser inspection",
        "resources": {
            "numeric_threads": pl.thread_pool_size(),
            "maximum_resident_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
            * (1 if sys.platform == "darwin" else 1024),
        },
    }
    manifest_path.write_text(json.dumps(report, indent=2) + "\n")
    return report


def quiet_coverage(quiet: dict, core: dict, reference: dict, frozen: dict) -> list[dict]:
    """Count complete support; reference-known counts remain on their native full grid."""
    if any(item.get("status") != "complete" for item in (quiet, core, reference)):
        raise ValueError("Quiet coverage requires completed panel manifests")
    if (
        reference.get("label") != "simulated refresh activity"
        or reference.get("actual_incumbent_complementarity_proven") is not False
    ):
        raise ValueError("Preserve the simulated reference identity")
    start = date.fromisoformat(frozen["range_start"])
    end = date.fromisoformat(frozen["range_end_exclusive"])
    dates = [str(start + timedelta(days=i)) for i in range((end - start).days)]
    indexed = []
    for manifest in (quiet, core, reference):
        entries = {day["date"]: day for day in manifest["days"]}
        if len(entries) != len(manifest["days"]) or set(entries) != set(dates):
            raise ValueError("Panel dates must uniquely cover the entire frozen calendar")
        indexed.append(entries)
    seconds = {b + offset for b in frozen["bucket_starts"] for offset in frozen["entry_offsets"]}
    seconds.update(frozen["additional_exit_seconds"])
    regular = {b + frozen["regular_entry_offset"] for b in frozen["bucket_starts"]}
    if len(regular) != 13 or not regular <= seconds or any(s >= 260 for s in regular):
        raise ValueError("Quiet arms require the frozen thirteen regular entry points")
    rows = []
    for day in dates:
        core_rows = indexed[1][day]["rows"]
        known = indexed[2][day]["known_rows"]
        if (
            type(core_rows) is not int
            or type(known) is not int
            or core_rows < 0
            or core_rows % len(seconds)
            or not 0 <= known <= core_rows
        ):
            raise ValueError("Invalid core schedule or reference availability count")
        scheduled = core_rows // len(seconds) * len(regular)
        artifacts = indexed[0][day]["artifacts"]
        identities = [(a["candidate"], a["arm"]) for a in artifacts]
        expected = [("quiet_explorer", "primary"), ("quiet_explorer", "reference_disagreement")]
        if sorted(identities) != sorted(expected):
            raise ValueError("Exactly the two declared quiet arms are required")
        for artifact in sorted(artifacts, key=lambda a: a["arm"]):
            complete = artifact["rows"]
            if type(complete) is not int or not 0 <= complete <= min(scheduled, known):
                raise ValueError("Complete support exceeds scheduled or known reference points")
            rows.append(
                {
                    "date": day,
                    "candidate": artifact["candidate"],
                    "arm": artifact["arm"],
                    "complete_rows": complete,
                    "scheduled_regular_points": scheduled,
                    "complete_fraction": complete / scheduled if scheduled else None,
                    "core_points": core_rows,
                    "reference_known_core_points": known,
                    "reference_status": "unknown"
                    if not known
                    else "partial"
                    if known < core_rows
                    else "known",
                }
            )
    return rows


def quiet_figure(rows: list[dict], frozen: dict) -> go.Figure:
    days = sorted({r["date"] for r in rows})
    arms = ["primary", "reference_disagreement"]
    fractions, details = [], []
    for arm in arms:
        arm_rows = [r for r in rows if r["arm"] == arm]
        fractions.append([r["complete_fraction"] for r in arm_rows])
        details.append(
            [
                f"quiet_explorer / {arm}<br>{r['date']}"
                f"<br>Measured complete rows: {r['complete_rows']}"
                f"<br>Scheduled regular points: {r['scheduled_regular_points']}"
                f"<br>Complete fraction: {r['complete_fraction']:.4%}"
                if r["complete_fraction"] is not None
                else f"quiet_explorer / {arm}<br>{r['date']}<br>No scheduled regular points; fraction undefined"
                for r in arm_rows
            ]
        )
        for i, row in enumerate(arm_rows):
            details[-1][i] += (
                f"<br>Reference availability: {row['reference_status']}"
                f"<br>Known reference / all stored points: {row['reference_known_core_points']} / {row['core_points']}"
                "<br>Zero complete support does not establish observed quiet."
            )
    fig = go.Figure(
        go.Heatmap(
            x=days,
            y=[f"quiet_explorer / {arm}" for arm in arms],
            z=fractions,
            zmin=0,
            zmax=1,
            colorscale=[[0, "#f0d7cf"], [0.5, "#e6b65c"], [1, "#258882"]],
            customdata=details,
            hovertemplate="%{customdata}<extra></extra>",
            colorbar={"title": "Complete / scheduled", "tickformat": ".0%", "thickness": 14},
            ygap=2,
        )
    )
    # A separate row preserves the unknown-reference distinction independently of zero support.
    reference = [r for r in rows if r["arm"] == "primary"]
    fig.add_trace(
        go.Scatter(
            x=days,
            y=["Simulated reference availability · full stored grid"] * len(days),
            mode="markers",
            marker={
                "symbol": "square",
                "size": 9,
                "color": [
                    {"unknown": "#d8dde5", "partial": "#aa81ba", "known": "#258882"}[
                        r["reference_status"]
                    ]
                    for r in reference
                ],
            },
            customdata=[
                f"{r['date']}<br>{r['reference_status']}: {r['reference_known_core_points']} / {r['core_points']} stored points"
                "<br>Gray: unknown; purple: partially known; teal: all known. No trade-activity inference."
                for r in reference
            ],
            hovertemplate="%{customdata}<extra></extra>",
            showlegend=False,
        )
    )
    fig.update_layout(
        title={
            "text": "Quiet explorer · measured complete-case daily coverage<br><sup>Two frozen arms · hover for exact rows · simulated reference only</sup>",
            "x": 0.015,
        },
        template="plotly_white",
        width=1700,
        height=660,
        margin={"l": 365, "r": 200, "t": 140, "b": 135},
        font={"family": "Arial, sans-serif", "size": 11},
    )
    fig.update_xaxes(
        type="date",
        title="UTC date · full frozen calendar",
        dtick="M1",
        tickformat="%b %Y",
        range=[frozen["range_start"], frozen["range_end_exclusive"]],
    )
    fig.update_yaxes(autorange="reversed")
    split_markers(fig, frozen)
    fig.add_annotation(
        x=0,
        y=-0.25,
        xref="paper",
        yref="paper",
        xanchor="left",
        showarrow=False,
        text="Zero complete rows = zero complete support, never observed quiet. Reference row: gray unknown, purple partial, teal all known.<br>"
        "Reference counts cover all stored points; arm fractions cover scheduled regular entries. Solid/dotted lines retain frozen fold boundaries.",
        font={"size": 10},
    )
    return fig


def preserve_or_write(path: Path, content: str) -> None:
    """Recover an identical interrupted output without replacing existing evidence."""
    payload = content.encode()
    if path.exists():
        if path.read_bytes() != payload:
            raise ValueError(f"Preserve incompatible existing output: {path}")
        return
    with path.open("xb") as handle:
        handle.write(payload)


def build_quiet_supplement(run: Path) -> dict:
    if not run.is_dir() or not str(run.resolve()).startswith("/Volumes/docker-data/"):
        raise ValueError("Existing canonical SSD run required")
    if pl.thread_pool_size() > 2:
        raise ValueError("Set POLARS_MAX_THREADS=2 before running coverage charts")
    names = ["quiet-complete-case-panels.json", "core-panels.json", "simulated-refresh-panels.json"]
    paths = [run / "manifests" / name for name in names]
    paths.extend(
        [run / "inputs/tournament-freeze.json", run / "diagnostics/source-coverage/manifest.json"]
    )
    documents = [json.loads(path.read_text()) for path in paths]
    quiet, core, reference, frozen, original = documents
    digests = [sha256(path) for path in paths]
    if (
        quiet["identity"]["source_panels"]["core-panels.json"] != digests[1]
        or quiet["identity"]["source_panels"]["simulated-refresh-panels.json"] != digests[2]
        or reference["identity"]["core_panels"] != digests[1]
        or reference["identity"]["configuration"] != digests[3]
        or core["configuration_sha256"] != digests[3]
    ):
        raise ValueError("Quiet source manifest provenance mismatch")
    if original["status"] != "complete" or original["plotly_version"] != plotly.__version__:
        raise ValueError("Existing coverage figure asset must remain compatible")
    out = run / "diagnostics/source-coverage"
    asset = out / "plotly.min.js"
    asset_record = [item for item in original["outputs"] if item["path"] == str(asset)]
    if len(asset_record) != 1 or sha256(asset) != asset_record[0]["sha256"]:
        raise ValueError("Original plotting asset changed")
    identity = {
        "inputs": [
            {"path": str(p), "sha256": digest} for p, digest in zip(paths, digests, strict=True)
        ],
        "implementation_sha256": sha256(Path(__file__)),
        "plotly_asset": asset_record[0],
    }
    destination = out / "quiet-arm-calendar.html"
    manifest_path = out / "quiet-manifest.json"
    if manifest_path.exists():
        saved = json.loads(manifest_path.read_text())
        if saved["identity"] != identity or saved["outputs"] != [
            {
                "path": str(destination),
                "sha256": sha256(destination),
                "type": "offline_plotly_figure",
            }
        ]:
            raise ValueError("Existing quiet supplement identity or output changed")
        return saved
    rows = quiet_coverage(quiet, core, reference, frozen)
    fig = quiet_figure(rows, frozen)
    html = fig.to_html(
        include_plotlyjs="plotly.min.js",
        div_id="quiet-arm-calendar",
        config={"displaylogo": False, "toImageButtonOptions": {"format": "svg"}},
    )
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * (
        1 if sys.platform == "darwin" else 1024
    )
    if peak > 512 * 1024**2:
        raise RuntimeError("Coverage chart process exceeded 512 MiB budget")
    preserve_or_write(destination, html)
    report = {
        "schema_version": "btc-quiet-coverage-supplement-v1",
        "status": "complete",
        "created_at": datetime.now(UTC).isoformat(),
        "identity": identity,
        "days": len(rows) // 2,
        "arms": 2,
        "source_products": 0,
        "daily_coverage": rows,
        "boundaries": frozen["folds"],
        "totals": {
            arm: {
                "complete_rows": sum(r["complete_rows"] for r in rows if r["arm"] == arm),
                "scheduled_regular_points": sum(
                    r["scheduled_regular_points"] for r in rows if r["arm"] == arm
                ),
            }
            for arm in ("primary", "reference_disagreement")
        },
        "denominator": "core rows / distinct frozen stored seconds * thirteen regular entry points; exact divisibility required",
        "outputs": [
            {
                "path": str(destination),
                "sha256": sha256(destination),
                "type": "offline_plotly_figure",
            }
        ],
        "limitations": [
            "Counts and artifact identities come from completed manifests; no raw data, model, outcome, or dataset content is read or rehashed.",
            "Zero complete support is measured coverage, never proof of observed quiet or no trades.",
            "Reference-known counts describe all stored core points; regular-entry reference-known counts cannot be inferred from these daily totals.",
            "Simulated refresh activity does not establish actual incumbent complementarity.",
            "The earlier arm calendar retains its original pre-reference pending state and is not revised.",
        ],
        "visual_inspection": "unavailable; no successful local visual render was inspected",
        "resources": {"numeric_threads": pl.thread_pool_size(), "maximum_resident_bytes": peak},
    }
    preserve_or_write(manifest_path, json.dumps(report, indent=2) + "\n")
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument(
        "--quiet-supplement",
        action="store_true",
        help="Add separate measured quiet coverage from completed panel manifests",
    )
    args = parser.parse_args()
    result = build_quiet_supplement(args.run) if args.quiet_supplement else build(args.run)
    print(
        json.dumps(
            {
                key: result[key]
                for key in ("status", "days", "source_products", "arms", "resources")
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
