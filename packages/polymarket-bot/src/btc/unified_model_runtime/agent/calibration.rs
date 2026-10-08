//! Bounded historical reliability context, never an admission rule or probability override.
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const HISTORY_SQL: &str = r#"
WITH runs AS MATERIALIZED (
 SELECT event_id AS run_id FROM polymarket.trading_process_events
 WHERE process_id=$1 AND event_type='btc_run_manifest' AND timestamp_utc < $2
 ORDER BY timestamp_utc DESC, event_id DESC LIMIT 8
), recent AS MATERIALIZED (
 SELECT d.* FROM runs CROSS JOIN LATERAL (
   SELECT market_id, decision_at, decision_id, status,
     (metadata->'agent_evaluation') #- '{request,context}' AS e
   FROM polymarket.btc_strategy_decisions
   WHERE run_id=runs.run_id AND process_id=$1
     AND decision_at >= $2 - interval '24 hours' AND decision_at < $2
     AND strategy_version='btc_5m_openai_agent_v1'
   ORDER BY decision_at DESC LIMIT 5001
 ) d
 ORDER BY decision_at DESC, decision_id DESC LIMIT 5001
), eligible AS (
 SELECT *, count(*) OVER() AS inspected_rows FROM recent
), forecasts AS (
 SELECT e#>>'{request,request_id}' AS request_id, market_id,
   e#>>'{request,selection,profile_key}' AS profile_key,
   e#>>'{request,selection,profile_sha256}' AS profile_sha256,
   (e#>>'{request,observed_at}')::timestamptz AS observed_at,
   e#>>'{prediction,Ok,direction}' AS direction,
   (e#>>'{prediction,Ok,probability_up}')::double precision AS probability_up,
   bool_or(status='filled') AS filled, max(inspected_rows) AS inspected_rows
 FROM eligible
 WHERE e#>'{prediction,Ok}' IS NOT NULL
   AND (e#>>'{request,observed_at}')::timestamptz < $2
   AND (e->>'completed_at')::timestamptz < $2
   AND (e#>>'{request,window_start}')::timestamptz >= $2 - interval '24 hours'
   AND (e#>>'{request,window_start}')::timestamptz + interval '5 minutes' <= $2
   AND (e#>>'{request,selection,profile_key}',e#>>'{request,selection,profile_sha256}')
       IN (SELECT * FROM unnest($3::text[], $4::text[]))
   -- Exclude the potentially partial oldest market when the row bound is reached.
   AND (inspected_rows < 5001 OR market_id <> (SELECT market_id FROM recent ORDER BY decision_at LIMIT 1))
 GROUP BY 1,2,3,4,5,6,7
), ranked AS (
 SELECT *, row_number() OVER(PARTITION BY market_id,profile_key ORDER BY observed_at,request_id) AS first_rank,
   row_number() OVER(PARTITION BY market_id,profile_key ORDER BY observed_at DESC,request_id DESC) AS last_rank
 FROM forecasts
)
SELECT jsonb_build_object('market_id',f.market_id,'profile_key',f.profile_key,
 'profile_sha256',f.profile_sha256,'observed_at',f.observed_at,
 'stage',CASE WHEN first_rank=1 THEN 'initial' ELSE 'latest_reassessment' END,
 'direction',direction,'probability_up',probability_up,'filled',filled,
 'outcome',r.outcome,'inspected_rows',inspected_rows)
FROM ranked f
LEFT JOIN LATERAL (
 SELECT lower(winning_outcome) AS outcome
 FROM market_data.polymarket_btc_five_minute_resolutions
 WHERE market_id=f.market_id AND received_at < $2 AND ingested_at < $2
   AND provider_available_at < $2 AND window_end <= $2
 ORDER BY received_at DESC, ingested_at DESC, revision_sha256 DESC LIMIT 1
) r ON true
WHERE first_rank=1 OR last_rank=1
ORDER BY f.observed_at,request_id
"#;

pub fn unavailable(cutoff: DateTime<Utc>) -> Value {
    json!({"version":"agent-historical-calibration-v1", "status":"unavailable",
        "cutoff":cutoff,"reason":"history_read_unavailable","groups":[]})
}

#[derive(Default)]
struct Band {
    markets: BTreeSet<String>,
    sum: f64,
    wins: usize,
}

pub fn summarize(cutoff: DateTime<Utc>, rows: Vec<Value>) -> Value {
    let mut groups: BTreeMap<(String, String, String, bool, u8), Band> = BTreeMap::new();
    let mut unresolved = 0;
    let mut inspected = 0;
    let mut earliest: Option<String> = None;
    for row in rows {
        inspected = inspected.max(row["inspected_rows"].as_u64().unwrap_or(0));
        let Some(p) = row["probability_up"]
            .as_f64()
            .filter(|p| (0.0..=1.0).contains(p))
        else {
            continue;
        };
        let Some(outcome) = row["outcome"]
            .as_str()
            .filter(|o| matches!(*o, "up" | "down"))
        else {
            unresolved += 1;
            continue;
        };
        let direction = row["direction"].as_str().unwrap_or("");
        if !matches!(direction, "up" | "down") || (direction == "up") != (p >= 0.5) {
            continue;
        }
        let probability = if direction == "up" { p } else { 1.0 - p };
        let band = ((probability * 10.0).floor() as u8).clamp(5, 9);
        let group = groups
            .entry((
                row["profile_key"].as_str().unwrap_or("").into(),
                row["profile_sha256"].as_str().unwrap_or("").into(),
                row["stage"].as_str().unwrap_or("").into(),
                row["filled"].as_bool().unwrap_or(false),
                band,
            ))
            .or_default();
        if !group
            .markets
            .insert(row["market_id"].as_str().unwrap_or("").into())
        {
            continue;
        }
        group.sum += probability;
        group.wins += usize::from(direction == outcome);
        if let Some(at) = row["observed_at"].as_str() {
            if earliest.as_ref().is_none_or(|old| at < old.as_str()) {
                earliest = Some(at.into());
            }
        }
    }
    let groups: Vec<_> = groups.into_iter().map(|((profile,sha,stage,filled,band),g)| {
        let n = g.markets.len() as f64;
        let rate = g.wins as f64 / n;
        // Wilson interval; one observation per market within each group.
        let z2 = 1.96_f64.powi(2);
        let center = (rate + z2/(2.0*n))/(1.0+z2/n);
        let half = 1.96*(rate*(1.0-rate)/n+z2/(4.0*n*n)).sqrt()/(1.0+z2/n);
        json!({"source_profile":profile,"source_profile_sha256":sha,"stage":stage,
            "filled":filled,"selected_probability_band":[f64::from(band)/10.0,f64::from(band+1)/10.0],
            "forecast_count":g.markets.len(),"distinct_markets":g.markets.len(),
            "mean_stated_probability":g.sum/n,"wins":g.wins,"observed_win_rate":rate,
            "wilson_95_interval":[center-half,center+half]})
    }).collect();
    json!({"version":"agent-historical-calibration-v1",
        "status":if groups.is_empty(){"insufficient_history"}else{"available"},
        "cutoff":cutoff,"lookback_hours":24,"decision_row_limit":5000,"latest_run_limit":8,
        "row_limit_reached":inspected>=5001,"earliest_resolved_forecast":earliest,
        "unresolved_representatives":unresolved,
        "representative_rule":"first and latest subsequent valid forecast per market and profile; polling duplicates removed",
        "interval_note":"Within-group binomial intervals do not account for dependence across markets. Initial and reassessment groups overlap; small samples are uncertain. Bands are lower-inclusive, upper-exclusive except 1.0.",
        "groups":groups})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_side_rates_deduplicate_and_keep_cohorts_separate() {
        let at = Utc::now();
        let row = json!({"market_id":"a","profile_key":"prior","profile_sha256":"hash",
            "stage":"initial","filled":false,"direction":"down","probability_up":0.2,
            "outcome":"down","inspected_rows":5001});
        let mut other = row.clone();
        other["market_id"] = json!("b");
        other["outcome"] = json!("up");
        let mut filled = row.clone();
        filled["filled"] = json!(true);
        filled["market_id"] = json!("c");
        let mut unresolved = row.clone();
        unresolved["outcome"] = Value::Null;
        let summary = summarize(at, vec![row.clone(), row, other, filled, unresolved]);
        let groups = summary["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["distinct_markets"], 2);
        assert_eq!(groups[0]["observed_win_rate"], 0.5);
        assert_eq!(groups[0]["mean_stated_probability"], 0.8);
        assert_eq!(summary["unresolved_representatives"], 1);
        assert_eq!(summary["row_limit_reached"], true);
        let interval = groups[0]["wilson_95_interval"].as_array().unwrap();
        assert!(interval[0].as_f64().unwrap() < 0.1 && interval[1].as_f64().unwrap() > 0.9);
    }
    #[test]
    fn absent_history_is_explicit_and_never_fabricates_zero_accuracy() {
        let at = Utc::now();
        assert_eq!(summarize(at, vec![])["status"], "insufficient_history");
        assert_eq!(unavailable(at)["status"], "unavailable");
        assert!(unavailable(at)["groups"].as_array().unwrap().is_empty());
    }
}
