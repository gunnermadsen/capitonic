# Execution configuration contract

Sources are required exclusively at `config.raw.sources` (`/raw/sources` within configuration and frozen snapshots). Source selectors are parsed separately from strategy controls. The `config.execution` and `config.raw.btc_realtime_paper` envelopes retain their other settings. Mode, execute_signals, live_capital and account_ref remain at their existing paths because account reconciliation SQL and dashboards consume them. New order rules live exclusively in `execution.order_policies`. The explicit rail selector and `btc_realtime_paper_process_v6` must agree. Any new marker selects strict validation, never legacy fallback.

Examples are executable compiler fixtures: [paper](examples/paper.json), [live](examples/live.json). They describe inactive definitions, not approved process creations, deployment configuration, or real-capital authorization. Preregistration values identify fixtures; conversion retains actual process provenance and canonical process_id. Keep legacy schemas and defaults unchanged.

## Resolution

Each policy is `{name, version: 1, params}`. Names are unique. Unknown names, fields and versions are rejected. Required policies are sizing, entry_timing, evidence, liquidity, execution_cost, edge, order_execution, exposure and loss_limit. Paper additionally requires paper_execution; live rejects it. Loss-regime is optional. Nullable exposure/loss limits and nullable high-water controls remain unconfigured when null; optional absence must not activate a new check. There are no policy enabled toggles.

Array ordering has no execution meaning. Member model-array order remains meaningful for first-qualified routing. Sources are canonicalized by key and policy parameters compile into a typed struct before hashing. Hash identity includes execution authority, source/inference/runtime settings and run provenance. Changing a policy changes identity. Decimal prices/money/fractions are strings; share bounds/default are JSON numbers converted to Decimal. Comparisons use Decimal. Artifact model identities remain checksummed and use existing UMR catalog/pinning validation.

The new compiler is pure apart from read-only model/profile loading and caching. Existing process PUT/PATCH calls pin/validate before the existing persistence calls. No migration creates processes. Runtime receives compiled settings, not stored JSON. Both UMR and API triggers are representable; neither starts while execution is unavailable. The existing lifecycle API owns enabled/status and rejects definition writes that try to activate a process.

## Complete parameter migration inventory

Paths below are relative to `config.raw.btc_realtime_paper`, except `execution.*` which is relative to `config`.

| Existing parameter / default | New owner | Meaning and compatibility |
| --- | --- | --- |
| strategy.target_size / required in persisted definitions | sizing_policy.default_share | Positive requested shares; new explicit lower/default/upper relationship. Fixed uses default. Dynamic rejects until decision semantics exist; never clamp upward to lower bound. |
| strategy.min_entry_price / 0.05; max_entry_price / 0.95 | sizing_policy | Strict 0 < min < max < 1. Selected model may independently impose a narrower frozen maximum. |
| strategy.max_depth_participation / 0.25 | sizing_policy | Fraction in (0,1]; affects admitted requested quantity. Paper clipping versus live rejection is an execution parity item, not repaired here. |
| strategy.min_seconds_after_open / 15; min_seconds_before_close / 20 | entry_timing_policy | Nonnegative open, positive close offset; nonempty 300-second window, preserving existing fixed-120 exception. UMR preserves model slot cadence and intersection. Exact inclusive/exclusive runtime comparisons stay with their current owners until shared execution implementation. |
| strategy.max_reference_age_ms / 2000 | evidence_policy | Applicable reference execution readiness. Preserve current model-dependent applicability. |
| strategy.max_chainlink_open_delay_ms / 5000 | evidence_policy | Opening-reference selection/evidence; no new mandatory model input. |
| strategy.max_book_age_ms / 2000 | evidence_policy | Current execution-book freshness, distinct from frozen causal feature-book freshness. |
| strategy.max_source_skew_ms / 1000 | evidence_policy | Existing applicable reference skew, not an extra gate for exempt model paths. |
| strategy.max_fee_age_ms / 3600000 | evidence_policy | Fee evidence age, using existing fee source. |
| execution.require_exit_book / optional | liquidity_policy.require_exit_book | Explicit boolean for the new contract, with equivalent paper/live book validation to be implemented. |
| strategy.spread_reserve_fraction / 0.10 | execution_cost_policy | Existing nonnegative spread reserve calculation. |
| strategy.slippage_reserve_bps / 25 | execution_cost_policy | Nonnegative basis points. |
| strategy.latency_reserve_per_share / 0.005 | execution_cost_policy | Nonnegative reserve per share. |
| strategy.max_fee_rate / 1 | execution_cost_policy | Nonnegative maximum observed fee rate, not a configured replacement fee. |
| strategy.min_net_edge_per_share / 0.015; min_net_edge_usd / 0.02 | edge_policy | Preserve asymmetric-value applicability. Do not newly activate these in directional admission. |
| paper.directional_model_entry_policy / require_positive_direct_edge | edge_policy | Shared directional/agent behavior despite its old paper namespace. Execute-directional mode does not waive other gates or frozen qualification. |
| strategy.decision_strategy.models[].entry_order_type / fok | order_execution_policy.member_order_types | Exact member_id map; only fok/fak. No duplicate value inside router members. |
| execution.max_open_positions / null | exposure_policy | Null or existing 1..6 configuration range. Existing execution/accounting semantics preserved. |
| execution.max_open_notional_usd / null | exposure_policy | Null or existing (0,30] USD configuration range. |
| execution.max_daily_loss_usd / null | loss_limit_policy | Null or any positive representable Decimal, with no hardcoded maximum. Shared paper/live OMS enforces the configured UTC daily realized-loss threshold. Legacy flat configuration has the same accepted range. |
| entry_admission.daily_realized_pnl_high_water_mark / absent | loss_limit_policy.daily_realized_pnl_high_water_mark | Reuse existing schema, activation_realized_pnl_usd and max_drawdown_from_high_water_mark_usd. Preserve UTC-period/evidence semantics; validate against paper starting collateral only for paper. |
| entry_admission.loss_regime_confidence_floor / absent | loss_regime_policy | Reuse schema_version, activation_consecutive_candidate_losses (1..100), min_conservative_probability ([0,1]), release_consecutive_candidate_wins (1..100). Candidate regime, not a newly invented executed-loss counter. |
| paper.arrival_latency_ms / 150 | paper_execution_policy | Existing 1..60000ms supported range. |
| paper.visible_depth_haircut / 0.80 | paper_execution_policy | (0,1]; simulation assumption, not an admission bypass. |
| paper.starting_collateral_usd / 1000 | paper_execution_policy | Existing (0,1000000] bound; initial ledger seed only. |
| paper.stress_previews / 300ms at .65, 600ms at .50 | paper_execution_policy | Unique scenario_key, arrival_latency_ms, visible_depth_haircut; no primary-ledger mutations. |

The compiler retains existing hard-coded configuration ceilings; changing them is not implicit in reorganizing parameters. Existing live reference/book freshness ceiling is 2000ms. Shared execution parity will use process policies within their documented domain constraints. Do not claim these code-owned ceilings are ML parameters.

## Retained or intentionally excluded

- `strategy.max_directional_feature_age_ms` remains inference configuration; null derives the artifact cadence, explicit values cannot exceed the supported cadence. `strategy.required_model_feeds` remains the existing feed-requirement contract. Neither appears in order policies.
- `strategy.decision_strategy`, risk_strategies, runtime.strategy_interval_ms (1..60000), run key and preregistration stay in their existing ownership domains. Rail-selected definitions always support UMR execution and authenticated API overrides; there is no trigger selector.
- `strategy_version`, `feature_schema_version`, unified_model bindings and artifact policy are derived, not configurable copies. Existing UMR compilation/pinning is reused for validation, never persisted as a legacy execution configuration.
- `execution.taker_fee_rate` is already rejected for BTC; no new override is added. Fees use observed market evidence.
- `max_order_notional_usd` is intentionally retired for new sizing; funds, reservations and aggregate USD accounting remain mandatory. Legacy values are not removed.
- `max_probability_uncertainty` currently has config validation but no runtime uncertainty comparison; no new policy activates it. Historical readers remain unchanged.
- New definitions reject old strategy order fields, entry_admission, paper controls and member entry_order_type. Non-null flat execution limits are rejected; null legacy Option fields carry no policy authority.
- Fixed policies do not certify historical model profitability at a different quantity. Frozen 5-share feature mathematics and historical qualified_trade_size remain untouched.

## Frozen model restrictions

Current conservative-selective original paper/live artifacts require conservative confidence .88; the .75 pair requires .75. Calibration subtracts .02 reliability; model maximum share cost .55, execution reserve .005, stress slippage .01 and minimum stressed edge .10 remain frozen. Slots are 30..210 seconds at 5-second cadence. Binance declared lookback is 301 seconds with 5000ms maximum age; causal books declare 2 seconds/2000ms. Deployment metadata controls live artifact authorization. These cannot be overridden by order policies. Their values must be exposed as qualification evidence in the future execution receipts.

## Compatibility and cutover

No automatic conversion. Existing process identity, enabled intent, sources and effective applicable values must be preserved through an authorized API update. Current old binaries do not accept the new fields: rollback needs compatible code AND configuration. In-flight orders retain originating implementation and configuration identity. Older 3.3.x changes are integrated only with authorization and must pass both legacy and new contract suites. No Helm/build/CI changes are part of this foundation.
