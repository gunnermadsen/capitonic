WITH market_facts AS MATERIALIZED (
  SELECT
    fact.market_id,
    max(fact.value) FILTER (WHERE fact.fact_type = 'opening_boundary') AS opening_boundary,
    max(fact.value) FILTER (WHERE fact.fact_type = 'final_price') AS final_price,
    count(*) FILTER (WHERE fact.fact_type = 'opening_boundary') AS opening_fact_count,
    count(*) FILTER (WHERE fact.fact_type = 'final_price') AS final_fact_count
  FROM polymarket.btc_market_reference_facts fact
  JOIN ingester.backfill_artifacts artifact
    ON artifact.artifact_id = fact.artifact_id
   AND artifact.status = 'completed'
  WHERE fact.source_effective_at >= %(batch_start)s
    AND fact.source_effective_at <= %(batch_end)s
    AND fact.fact_type IN ('opening_boundary', 'final_price')
  GROUP BY fact.market_id
)
SELECT
  market.market_id,
  market.window_start,
  market.window_end,
  market.official_outcome,
  CASE WHEN market.official_outcome = 'up' THEN 1 ELSE 0 END AS label_up,
  facts.opening_boundary::double precision AS opening_boundary,
  facts.final_price::double precision AS final_price
FROM polymarket.btc_interval_markets market
JOIN market_facts facts USING (market_id)
WHERE market.window_start >= %(batch_start)s
  AND market.window_start < %(batch_end)s
  AND market.validation_status = 'valid'
  AND market.official_outcome IN ('up', 'down')
  AND facts.opening_fact_count = 1
  AND facts.final_fact_count <= 1
  AND (%(strict_final_price_audit)s = false OR facts.final_fact_count = 1)
ORDER BY market.window_start, market.market_id;
