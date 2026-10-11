//! Agent-only probability calibration and current-book evidence, using the existing entry seam.
use super::EvaluationResult;
use crate::btc::strategy::BtcFeatureSnapshot;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

// Read only filled candidates through the existing run/process index. Decode payloads
// after the bound; repeated 250ms rejected decisions cannot evict this population.
pub const HISTORY_SQL: &str = r#"
WITH runs AS MATERIALIZED (
 SELECT event_id AS run_id FROM polymarket.trading_process_events
 WHERE process_id=$1 AND event_type='btc_run_manifest' AND timestamp_utc<$2
 ORDER BY timestamp_utc DESC LIMIT 8
), bounded AS MATERIALIZED (
 SELECT d.* FROM runs CROSS JOIN LATERAL (
 SELECT market_id, snapshot_id, decision_at, executable_price, fee_per_share, outcome, metadata
 FROM polymarket.btc_strategy_decisions
 WHERE run_id=runs.run_id AND process_id=$1 AND action='buy' AND status='filled'
 AND decision_at >= $2-interval '24 hours' AND decision_at<$2
 ORDER BY decision_at DESC LIMIT 301
 ) d ORDER BY decision_at DESC LIMIT 301
)
SELECT jsonb_build_object('market',d.market_id,'at',d.decision_at,
 'profile',e#>>'{request,selection,profile_key}',
 'sha',e#>>'{request,selection,profile_sha256}',
 'p',e#>'{prediction,Ok,probability_up}', 'direction',d.outcome,
 'request_up',e#>'{request,context,execution_snapshot,up_book,executable_ask_vwap}',
 'request_down',e#>'{request,context,execution_snapshot,down_book,executable_ask_vwap}',
 'entry_up',s.features#>'{up_book,executable_ask_vwap}',
 'entry_down',s.features#>'{down_book,executable_ask_vwap}',
 'cost',d.executable_price+d.fee_per_share,
 'price',d.executable_price,
 'elapsed',extract(epoch from d.decision_at-(e#>>'{request,window_start}')::timestamptz),
 'outcome',r.outcome)
FROM bounded d
JOIN LATERAL (
 SELECT features FROM polymarket.btc_feature_snapshots
 WHERE snapshot_id=d.snapshot_id AND feature_as_of=d.decision_at
 AND feature_as_of >= $2-interval '24 hours' AND feature_as_of<$2
 LIMIT 1
) s ON true
CROSS JOIN LATERAL (SELECT d.metadata->'agent_evaluation' e) x
LEFT JOIN LATERAL (
 SELECT lower(winning_outcome) outcome FROM market_data.polymarket_btc_five_minute_resolutions
 WHERE market_id=d.market_id AND received_at<$2 AND ingested_at<$2
 AND provider_available_at<$2 AND window_end<=$2
 ORDER BY received_at DESC,ingested_at DESC,revision_sha256 DESC LIMIT 1
) r ON true
WHERE (e#>>'{request,selection,profile_key}',e#>>'{request,selection,profile_sha256}')
 IN (SELECT * FROM unnest($3::text[],$4::text[]))
 AND (e->>'completed_at')::timestamptz<$2
ORDER BY d.decision_at
"#;

pub(super) fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
fn prior(up: f64, down: f64) -> Option<f64> {
    ((0.0..1.0).contains(&up) && (0.0..1.0).contains(&down) && up > 0.0 && down > 0.0)
        .then_some(up / (up + down))
}
fn logit(p: f64) -> f64 {
    let p = p.clamp(0.000001, 0.999999);
    (p / (1.0 - p)).ln()
}
fn transport(p: f64, before: f64, now: f64) -> f64 {
    1.0 / (1.0 + (-(logit(p) + logit(now) - logit(before))).exp())
}

#[derive(Clone)]
struct Sample {
    p: f64,
    market: f64,
    entry_market: f64,
    y: f64,
    cost: f64,
    up: bool,
}
fn brier(rows: &[Sample], weight: f64) -> f64 {
    rows.iter()
        .map(|r| {
            (transport(
                r.market + weight * (r.p - r.market),
                r.market,
                r.entry_market,
            ) - r.y)
                .powi(2)
        })
        .sum::<f64>()
        / rows.len() as f64
}
fn retained_pnl(rows: &[Sample], weight: f64) -> f64 {
    rows.iter()
        .map(|r| {
            let p = transport(
                r.market + weight * (r.p - r.market),
                r.market,
                r.entry_market,
            );
            let selected = if r.up { p } else { 1.0 - p };
            if (p >= 0.5) == r.up && selected > r.cost {
                (if r.up { r.y } else { 1.0 - r.y }) - r.cost
            } else {
                0.0
            }
        })
        .sum()
}

/// Fit only on the older 2/3; the untouched later 1/3 must improve Brier and
/// not worsen fee-net retained-entry PnL. No refit on the validation rows.
pub fn summarize(cutoff: chrono::DateTime<chrono::Utc>, rows: Vec<Value>) -> Value {
    let mut cohorts: BTreeMap<(String, String), Vec<Sample>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut bands: BTreeMap<(String, u8, bool, u8), (usize, usize)> = BTreeMap::new();
    let truncated = rows.len() >= 301;
    for row in rows {
        let Some((p, m, cost, price)) = (|| {
            Some((
                number(&row["p"])?,
                prior(number(&row["request_up"])?, number(&row["request_down"])?)?,
                number(&row["cost"])?,
                number(&row["price"])?,
            ))
        })() else {
            continue;
        };
        let Some(entry_market) = number(&row["entry_up"])
            .zip(number(&row["entry_down"]))
            .and_then(|(u, d)| prior(u, d))
        else {
            continue;
        };
        let Some(y) = row["outcome"]
            .as_str()
            .filter(|s| matches!(*s, "up" | "down"))
        else {
            continue;
        };
        if !(0.0..=1.0).contains(&p) || !(0.0..=1.0).contains(&cost) {
            continue;
        }
        let profile = row["profile"].as_str().unwrap_or("").to_string();
        let sha = row["sha"].as_str().unwrap_or("").to_string();
        if !seen.insert((profile.clone(), sha.clone(), row["market"].to_string())) {
            continue;
        }
        let up = row["direction"] == "up";
        let band = bands
            .entry((
                profile.clone(),
                (price * 10.0) as u8,
                (p >= 0.5) != (m >= 0.5),
                if number(&row["elapsed"]).unwrap_or(0.0) < 75.0 {
                    0
                } else {
                    1
                },
            ))
            .or_default();
        band.0 += 1;
        band.1 += usize::from(up == (y == "up"));
        cohorts.entry((profile, sha)).or_default().push(Sample {
            p,
            market: m,
            entry_market,
            y: if y == "up" { 1.0 } else { 0.0 },
            cost,
            up,
        });
    }
    let cohorts: Vec<_> = cohorts.into_iter().map(|((profile,sha),rows)| {
        let split=rows.len()*2/3;
        let (train,test)=rows.split_at(split);
        let mut value=json!({"source_profile":profile,"source_sha256":sha,"markets":rows.len(),"training_markets":train.len(),"holdout_markets":test.len(),"status":"insufficient_history","model_weight":1.0});
        if train.len()>=40 && test.len()>=20 && !truncated {
            let weight = [1.0,0.75,0.5,0.25,0.0].into_iter().min_by(|a,b|brier(train,*a).total_cmp(&brier(train,*b))).unwrap();
            let baseline=test.iter().map(|r|(r.p-r.y).powi(2)).sum::<f64>()/test.len() as f64; let candidate=brier(test,weight);
            let baseline_pnl=test.iter().map(|r|(if r.up {r.y}else{1.0-r.y})-r.cost).sum::<f64>(); let candidate_pnl=retained_pnl(test,weight);
            let qualified=candidate<baseline && candidate_pnl>=baseline_pnl;
            value["status"]=json!(if qualified {"qualified"}else{"holdout_not_improved"});
            value["model_weight"]=json!(if qualified {weight}else{1.0});
            value["holdout"]=json!({"raw_brier":baseline,"candidate_brier":candidate,"raw_quoted_pnl_per_share":baseline_pnl,"candidate_retained_quoted_pnl_per_share":candidate_pnl});
        }
        value
    }).collect();
    let bands:Vec<_>=bands.into_iter().map(|((profile,price,disagreement,time),(n,w))|json!({"profile":profile,"entry_price_band":[f64::from(price)/10.0,f64::from(price+1)/10.0],"disagrees_with_market":disagreement,"entry_seconds_band":if time==0 {[45,75]}else{[75,120]},"markets":n,"wins":w})).collect();
    json!({"version":"agent-entry-calibration-v1","cutoff":cutoff,"status":"available","row_limit_reached":truncated,"cohorts":cohorts,"bands":bands,"meaning":"Prior filled markets only; separate profiles. Chronological holdout is a qualification check, not proof of future profitability. PnL replays retention of observed entries only, not hypothetical opposite fills; quoted costs include fees but exclude arrival slippage."})
}

/// Pure and repeatable from the original raw forecast on every entry attempt;
/// never compound an adjustment on a retained or restored result.
pub fn adjust(result: &EvaluationResult, snapshot: &BtcFeatureSnapshot) -> Option<(f64, Value)> {
    if result.request.selection.profile_key != super::EXECUTION_EVIDENCE_PROFILE_KEY {
        return None;
    }
    let raw = result.prediction.as_ref().ok()?.probability_up;
    let context = &result.request.context;
    let before = prior(
        number(context.pointer("/execution_snapshot/up_book/executable_ask_vwap")?)?,
        number(context.pointer("/execution_snapshot/down_book/executable_ask_vwap")?)?,
    )?;
    let now = prior(
        snapshot.up_book.executable_ask_vwap?.to_f64()?,
        snapshot.down_book.executable_ask_vwap?.to_f64()?,
    )?;
    // Do not blend source cohorts. Prefer this profile; bootstrap only from the
    // immediately preceding independent-forecast profile with its pinned identity.
    let cohorts = context
        .pointer("/historical_calibration/entry_calibration/cohorts")
        .and_then(Value::as_array);
    let selected = [
        super::EXECUTION_EVIDENCE_PROFILE_KEY,
        super::FRESH_EVIDENCE_PROFILE_KEY,
    ]
    .iter()
    .find_map(|key| {
        cohorts?
            .iter()
            .find(|c| c["source_profile"] == *key && c["status"] == "qualified")
    });
    let weight = selected
        .and_then(|c| number(&c["model_weight"]))
        .filter(|w| (0.0..=1.0).contains(w))
        .unwrap_or(1.0);
    let calibrated = before + weight * (raw - before);
    let adjusted = transport(calibrated, before, now);
    Some((
        adjusted,
        json!({"raw_probability_up":raw,"calibrated_snapshot_probability_up":calibrated,"entry_probability_up":adjusted,"snapshot_market_probability_up":before,"entry_market_probability_up":now,"model_weight":weight,"calibration_applied":selected.is_some(),"calibration_source":selected,"snapshot_at":result.request.observed_at,"entry_at":snapshot.observed_at,"meaning":"Normalized two-sided executable asks are a market proxy, not a true probability. Preserve calibrated log-odds residual as that proxy changes; recompute direction from entry probability. Raw forecast remains unchanged."}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rows(holdout_good: bool) -> Vec<Value> {
        (0..60).map(|i|json!({"market":i.to_string(),"profile":super::super::FRESH_EVIDENCE_PROFILE_KEY,"sha":"pinned","p":0.8,"request_up":"0.3","request_down":"0.7","entry_up":"0.3","entry_down":"0.7","direction":"up","price":0.3,"cost":0.32,"elapsed":60,"outcome":if i>=40 && holdout_good {"up"}else{"down"}})).collect()
    }
    #[test]
    fn chronological_holdout_must_improve_and_small_samples_do_not_override() {
        let at = chrono::Utc::now();
        let qualified = summarize(at, rows(false));
        assert_eq!(qualified["cohorts"][0]["status"], "qualified");
        assert_eq!(qualified["cohorts"][0]["model_weight"], 0.0);
        let rejected = summarize(at, rows(true));
        assert_eq!(rejected["cohorts"][0]["status"], "holdout_not_improved");
        assert_eq!(rejected["cohorts"][0]["model_weight"], 1.0);
        assert_eq!(
            summarize(at, rows(false)[..59].to_vec())["cohorts"][0]["status"],
            "insufficient_history"
        );
        // Poll duplicates never increase independent market counts.
        let repeated = rows(false)[..20].iter().cycle().take(60).cloned().collect();
        assert_eq!(summarize(at, repeated)["cohorts"][0]["markets"], 20);
    }
    #[test]
    fn book_transport_is_symmetric_and_cannot_invent_a_static_probability_bargain() {
        assert!((transport(0.57, 0.49, 0.49) - 0.57).abs() < 1e-12);
        let fallen = transport(0.57, 0.49, 0.22);
        assert!(fallen < 0.30 && fallen > 0.22);
        assert!((transport(0.43, 0.51, 0.78) - (1.0 - fallen)).abs() < 1e-12);
        assert!(prior(0.0, 0.5).is_none());
        assert!(prior(f64::NAN, 0.5).is_none());
        assert!(transport(1.0, 0.5, 0.2).is_finite());
    }
    #[test]
    fn new_profile_preserves_frozen_identity() {
        assert_eq!(
            super::super::hash(
                &super::super::profile_for_key(super::super::FRESH_EVIDENCE_PROFILE_KEY).unwrap()
            )
            .unwrap(),
            "70ef31029dd420cfa3c613109b1bc6a58f7f349d46495b51ea0ebf69f615d53d"
        );
        let profile =
            super::super::profile_for_key(super::super::EXECUTION_EVIDENCE_PROFILE_KEY).unwrap();
        eprintln!(
            "execution evidence profile sha256: {}",
            super::super::hash(&profile).unwrap()
        );
        assert!(super::super::uses_market_context(profile.key));
        assert!(super::super::uses_calibration(profile.key));
        assert!(super::super::uses_entry_reassessment(profile.key));
    }
}
