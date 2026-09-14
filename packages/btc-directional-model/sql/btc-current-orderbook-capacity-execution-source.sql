WITH eligible_markets AS MATERIALIZED (
  SELECT
    market.market_id,
    market.window_start,
    market.window_end,
    CASE WHEN market.official_outcome = 'up' THEN 1 ELSE 0 END AS label_up,
    COALESCE(market.fee_rate, 0)::double precision AS fee_rate,
    market.up_token_id,
    market.down_token_id
  FROM polymarket.btc_interval_markets market
  WHERE market.window_start >= %(batch_start)s
    AND market.window_start < %(batch_end)s
    AND market.validation_status = 'valid'
    AND market.official_outcome IN ('up', 'down')
), points AS MATERIALIZED (
  SELECT
    market.*,
    offset_seconds::integer AS seconds_elapsed,
    market.window_start + make_interval(secs => offset_seconds) AS observed_at
  FROM eligible_markets market
  CROSS JOIN generate_series(30, 240, 5) offset_seconds
), books AS MATERIALIZED (
  SELECT
    point.*,
    side.outcome,
    snapshot.capture_artifact_id,
    snapshot.received_at,
    snapshot.best_ask::double precision AS best_ask,
    snapshot.asks
  FROM points point
  CROSS JOIN LATERAL (
    VALUES ('up'::text, point.up_token_id), ('down'::text, point.down_token_id)
  ) side(outcome, token_id)
  LEFT JOIN LATERAL (
    SELECT
      source.capture_artifact_id,
      source.received_at,
      source.best_ask,
      source.asks
    FROM polymarket.btc_five_minute_orderbook_snapshots source
    WHERE source.market_id = point.market_id
      AND source.token_id = side.token_id
      AND source.sampled_at <= point.observed_at
      AND source.sampled_at > point.observed_at - interval '2 seconds'
      AND source.received_at <= point.observed_at
      AND source.received_at > point.observed_at - interval '2 seconds'
    ORDER BY source.source_timestamp DESC, source.sampled_at DESC
    LIMIT 1
  ) snapshot ON true
), priced AS MATERIALIZED (
  SELECT
    book.*,
    ladder.ask_depth,
    ladder.ask_vwap_5,
    ladder.ask_vwap_10,
    ladder.ask_vwap_15,
    ladder.ask_vwap_20,
    ladder.ask_vwap_25,
    ladder.ask_vwap_30,
    ladder.ask_vwap_40,
    ladder.ask_vwap_50,
    ladder.ask_vwap_75,
    ladder.ask_vwap_100,
    ladder.ask_vwap_125,
    ladder.ask_vwap_150,
    ladder.ask_vwap_175,
    ladder.ask_vwap_200
  FROM books book
  LEFT JOIN LATERAL (
    SELECT
      sum(size)::double precision AS ask_depth,
      CASE WHEN sum(size) >= 5 THEN
        sum(price * least(size, greatest(5 - prior_size, 0.0))) / 5 END::double precision
        AS ask_vwap_5,
      CASE WHEN sum(size) >= 10 THEN
        sum(price * least(size, greatest(10 - prior_size, 0.0))) / 10 END::double precision
        AS ask_vwap_10,
      CASE WHEN sum(size) >= 15 THEN
        sum(price * least(size, greatest(15 - prior_size, 0.0))) / 15 END::double precision
        AS ask_vwap_15,
      CASE WHEN sum(size) >= 20 THEN
        sum(price * least(size, greatest(20 - prior_size, 0.0))) / 20 END::double precision
        AS ask_vwap_20,
      CASE WHEN sum(size) >= 25 THEN
        sum(price * least(size, greatest(25 - prior_size, 0.0))) / 25 END::double precision
        AS ask_vwap_25,
      CASE WHEN sum(size) >= 30 THEN
        sum(price * least(size, greatest(30 - prior_size, 0.0))) / 30 END::double precision
        AS ask_vwap_30,
      CASE WHEN sum(size) >= 40 THEN
        sum(price * least(size, greatest(40 - prior_size, 0.0))) / 40 END::double precision
        AS ask_vwap_40,
      CASE WHEN sum(size) >= 50 THEN
        sum(price * least(size, greatest(50 - prior_size, 0.0))) / 50 END::double precision
        AS ask_vwap_50,
      CASE WHEN sum(size) >= 75 THEN
        sum(price * least(size, greatest(75 - prior_size, 0.0))) / 75 END::double precision
        AS ask_vwap_75,
      CASE WHEN sum(size) >= 100 THEN
        sum(price * least(size, greatest(100 - prior_size, 0.0))) / 100 END::double precision
        AS ask_vwap_100,
      CASE WHEN sum(size) >= 125 THEN
        sum(price * least(size, greatest(125 - prior_size, 0.0))) / 125 END::double precision
        AS ask_vwap_125,
      CASE WHEN sum(size) >= 150 THEN
        sum(price * least(size, greatest(150 - prior_size, 0.0))) / 150 END::double precision
        AS ask_vwap_150,
      CASE WHEN sum(size) >= 175 THEN
        sum(price * least(size, greatest(175 - prior_size, 0.0))) / 175 END::double precision
        AS ask_vwap_175,
      CASE WHEN sum(size) >= 200 THEN
        sum(price * least(size, greatest(200 - prior_size, 0.0))) / 200 END::double precision
        AS ask_vwap_200
    FROM (
      SELECT
        (level->>0)::double precision AS price,
        (level->>1)::double precision AS size,
        COALESCE(
          sum((level->>1)::double precision) OVER (
            ORDER BY (level->>0)::double precision
            ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
          ),
          0.0
        ) AS prior_size
      FROM jsonb_array_elements(COALESCE(book.asks, '[]'::jsonb)) level
    ) levels
    WHERE prior_size < 200
  ) ladder ON true
)
SELECT
  market_id,
  window_start,
  window_end,
  label_up,
  fee_rate,
  observed_at,
  seconds_elapsed,
  max(capture_artifact_id::text) AS artifact_id,
  'btc5m-current-orderbook-30-240s-v1'::text AS schema_version,
  max(received_at) FILTER (WHERE outcome = 'up') AS up_provider_received_at,
  max(best_ask) FILTER (WHERE outcome = 'up') AS up_best_ask,
  max(ask_depth) FILTER (WHERE outcome = 'up') AS up_ask_depth,
  max(ask_vwap_5) FILTER (WHERE outcome = 'up') AS up_ask_vwap_5,
  max(ask_vwap_10) FILTER (WHERE outcome = 'up') AS up_ask_vwap_10,
  max(ask_vwap_15) FILTER (WHERE outcome = 'up') AS up_ask_vwap_15,
  max(ask_vwap_20) FILTER (WHERE outcome = 'up') AS up_ask_vwap_20,
  max(ask_vwap_25) FILTER (WHERE outcome = 'up') AS up_ask_vwap_25,
  max(ask_vwap_30) FILTER (WHERE outcome = 'up') AS up_ask_vwap_30,
  max(ask_vwap_40) FILTER (WHERE outcome = 'up') AS up_ask_vwap_40,
  max(ask_vwap_50) FILTER (WHERE outcome = 'up') AS up_ask_vwap_50,
  max(ask_vwap_75) FILTER (WHERE outcome = 'up') AS up_ask_vwap_75,
  max(ask_vwap_100) FILTER (WHERE outcome = 'up') AS up_ask_vwap_100,
  max(ask_vwap_125) FILTER (WHERE outcome = 'up') AS up_ask_vwap_125,
  max(ask_vwap_150) FILTER (WHERE outcome = 'up') AS up_ask_vwap_150,
  max(ask_vwap_175) FILTER (WHERE outcome = 'up') AS up_ask_vwap_175,
  max(ask_vwap_200) FILTER (WHERE outcome = 'up') AS up_ask_vwap_200,
  max(received_at) FILTER (WHERE outcome = 'down') AS down_provider_received_at,
  max(best_ask) FILTER (WHERE outcome = 'down') AS down_best_ask,
  max(ask_depth) FILTER (WHERE outcome = 'down') AS down_ask_depth,
  max(ask_vwap_5) FILTER (WHERE outcome = 'down') AS down_ask_vwap_5,
  max(ask_vwap_10) FILTER (WHERE outcome = 'down') AS down_ask_vwap_10,
  max(ask_vwap_15) FILTER (WHERE outcome = 'down') AS down_ask_vwap_15,
  max(ask_vwap_20) FILTER (WHERE outcome = 'down') AS down_ask_vwap_20,
  max(ask_vwap_25) FILTER (WHERE outcome = 'down') AS down_ask_vwap_25,
  max(ask_vwap_30) FILTER (WHERE outcome = 'down') AS down_ask_vwap_30,
  max(ask_vwap_40) FILTER (WHERE outcome = 'down') AS down_ask_vwap_40,
  max(ask_vwap_50) FILTER (WHERE outcome = 'down') AS down_ask_vwap_50,
  max(ask_vwap_75) FILTER (WHERE outcome = 'down') AS down_ask_vwap_75,
  max(ask_vwap_100) FILTER (WHERE outcome = 'down') AS down_ask_vwap_100,
  max(ask_vwap_125) FILTER (WHERE outcome = 'down') AS down_ask_vwap_125,
  max(ask_vwap_150) FILTER (WHERE outcome = 'down') AS down_ask_vwap_150,
  max(ask_vwap_175) FILTER (WHERE outcome = 'down') AS down_ask_vwap_175,
  max(ask_vwap_200) FILTER (WHERE outcome = 'down') AS down_ask_vwap_200,
  0::integer AS quality_flags
FROM priced
GROUP BY market_id, window_start, window_end, label_up, fee_rate, observed_at, seconds_elapsed
HAVING count(*) FILTER (WHERE received_at IS NOT NULL) = 2
ORDER BY market_id, observed_at;
