WITH opening_facts AS MATERIALIZED (
  SELECT DISTINCT ON (fact.market_id)
    fact.market_id,
    fact.value::double precision AS opening_boundary
  FROM polymarket.btc_market_reference_facts fact
  JOIN ingester.backfill_artifacts artifact
    ON artifact.artifact_id = fact.artifact_id
   AND artifact.status = 'completed'
  WHERE fact.fact_type = 'opening_boundary'
    AND fact.source_effective_at >= %(batch_start)s - interval '5 minutes'
    AND fact.source_effective_at < %(batch_end)s + interval '5 minutes'
  ORDER BY fact.market_id, fact.source_effective_at DESC, fact.created_at DESC
)
SELECT
  market.market_id,
  market.window_start,
  market.window_end,
  market.official_outcome,
  CASE
    WHEN market.official_outcome = 'up' THEN 1
    WHEN market.official_outcome = 'down' THEN 0
    ELSE NULL
  END AS official_label_up,
  COALESCE(fact.opening_boundary, market.reference_price::double precision) AS opening_boundary,
  market.resolution_price::double precision AS final_price
FROM polymarket.btc_interval_markets market
LEFT JOIN opening_facts fact USING (market_id)
WHERE market.window_start >= %(batch_start)s
  AND market.window_start < %(batch_end)s
  AND market.validation_status = 'valid'
  AND COALESCE(fact.opening_boundary, market.reference_price::double precision) > 0
ORDER BY market.window_start, market.market_id;
