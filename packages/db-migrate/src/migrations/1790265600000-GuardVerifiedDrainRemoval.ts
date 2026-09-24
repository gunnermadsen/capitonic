import { MigrationInterface, QueryRunner } from 'typeorm';

const datasets = [
  ['polymarket_btc_five_minute_orderbooks', 'polymarket', 'btc_five_minute_orderbook_snapshots', 'sampled_at'],
  ['binance_spot_btcusdt_one_second_ohlcv', 'market_data', 'binance_spot_btcusdt_one_second_ohlcv', 'open_timestamp'],
  ['pmdata_chainlink_btcusd_reference_price', 'market_data', 'pmdata_chainlink_btcusd_reference_prices', 'source_timestamp'],
  ['pmdata_chainlink_btcusd_twap', 'market_data', 'pmdata_chainlink_btcusd_twap', 'source_timestamp'],
  ['polymarket_chainlink_btcusd_twap', 'market_data', 'polymarket_chainlink_btcusd_twap', 'source_timestamp'],
  ['polymarket_reference_price_ticks', 'polymarket', 'reference_price_ticks', 'source_timestamp'],
  ['chainlink_btcusd_one_minute_candles', 'market_data', 'chainlink_btcusd_one_minute_candles', 'open_timestamp'],
  ['polymarket_btc_capacity_execution_snapshots', 'polymarket', 'btc_market_capacity_execution_snapshots', 'sampled_at'],
  ['polymarket_btc_feature_snapshots', 'polymarket', 'btc_feature_snapshots', 'feature_as_of'],
  ['binance_spot_btcusdt_l2_snapshots', 'market_data', 'binance_spot_btcusdt_l2_snapshots', 'source_timestamp'],
  ['polygon_chainlink_btcusd_oracle_rounds', 'market_data', 'polygon_chainlink_btcusd_oracle_rounds', 'source_timestamp'],
  ['binance_futures_btcusdt_l2_one_second_features', 'market_data', 'binance_futures_btcusdt_l2_one_second_features', 'second_start'],
  ['binance_spot_btcusdt_l2_one_second_features', 'market_data', 'binance_spot_btcusdt_l2_one_second_features', 'second_start'],
  ['binance_futures_btcusdt_open_interest', 'market_data', 'binance_futures_btcusdt_open_interest', 'source_timestamp'],
] as const;

const datasetCases = datasets.map(([key, schema, table, timestamp]) => `
          WHEN object_record.strategy_key = '${key}'
            AND object_record.source_relation = '${schema}.${table}'
          THEN target_relation := '${schema}.${table}'::regclass;
            target_schema := '${schema}'; target_name := '${table}';
            time_column := '${timestamp}';`).join('');

export class GuardVerifiedDrainRemoval1790265600000 implements MigrationInterface {
  name = 'GuardVerifiedDrainRemoval1790265600000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE INDEX idx_ingester_removed_drain_object_window
        ON ingester.drain_objects (strategy_key, source_start, source_end)
        WHERE status = 'removed';

      CREATE OR REPLACE FUNCTION ingester.reject_drained_history_insert()
      RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
      SET search_path = pg_catalog, ingester
      AS $$
      DECLARE row_time timestamptz;
      BEGIN
        CASE TG_ARGV[0]
          WHEN 'binance_spot_btcusdt_l2_snapshots' THEN row_time := NEW.source_timestamp;
          WHEN 'binance_spot_btcusdt_one_second_ohlcv' THEN row_time := NEW.open_timestamp;
          WHEN 'polymarket_btc_five_minute_orderbooks' THEN row_time := NEW.sampled_at;
          ELSE RAISE EXCEPTION 'unregistered drain insert guard target';
        END CASE;
        IF row_time < clock_timestamp() - interval '1 day' THEN
          PERFORM pg_advisory_xact_lock(hashtextextended(TG_ARGV[0], 0));
          IF EXISTS (
            SELECT 1 FROM ingester.drain_objects object
            WHERE object.strategy_key = TG_ARGV[0]
              AND object.status = 'removed'
              AND object.source_start <= row_time
              AND object.source_end > row_time
          ) THEN
            RAISE EXCEPTION 'historical source chunk has been drained';
          END IF;
        END IF;
        RETURN NEW;
      END
      $$;

      CREATE TRIGGER trg_reject_drained_binance_spot_l2_history
        BEFORE INSERT ON market_data.binance_spot_btcusdt_l2_snapshots
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_drained_history_insert(
          'binance_spot_btcusdt_l2_snapshots');
      CREATE TRIGGER trg_reject_drained_binance_one_second_history
        BEFORE INSERT ON market_data.binance_spot_btcusdt_one_second_ohlcv
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_drained_history_insert(
          'binance_spot_btcusdt_one_second_ohlcv');
      CREATE TRIGGER trg_reject_drained_polymarket_orderbook_history
        BEFORE INSERT ON polymarket.btc_five_minute_orderbook_snapshots
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_drained_history_insert(
          'polymarket_btc_five_minute_orderbooks');

      CREATE OR REPLACE FUNCTION ingester.remove_verified_drain_chunk(
        requested_object_id uuid, expected_sha256 text, requesting_job_id uuid
      ) RETURNS bigint
      LANGUAGE plpgsql SECURITY DEFINER
      SET search_path = pg_catalog, public, ingester, market_data, polymarket
      AS $$
      DECLARE
        object_record ingester.drain_objects%ROWTYPE;
        job_cutoff timestamptz;
        target_relation regclass;
        target_schema text;
        target_name text;
        time_column text;
        immutability_trigger text;
        matching_chunks integer;
        current_rows bigint;
        dropped_chunks integer;
      BEGIN
        SELECT object.* INTO object_record
        FROM ingester.drain_objects object
        WHERE object.object_id = requested_object_id
        FOR UPDATE;
        IF NOT FOUND OR object_record.status <> 'published'
          OR object_record.sha256 <> expected_sha256 THEN
          RAISE EXCEPTION 'drain object is not a verified publication';
        END IF;

        SELECT job.cutoff INTO job_cutoff
        FROM ingester.drain_jobs job
        WHERE job.job_id = requesting_job_id
          AND job.strategy_key = object_record.strategy_key
          AND job.status = 'running' AND job.mode = 'drain'
          AND job.cancel_requested_at IS NULL;
        IF NOT FOUND OR object_record.source_end > job_cutoff THEN
          RAISE EXCEPTION 'source chunk is outside the running drain cutoff';
        END IF;

        CASE${datasetCases}
          ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';
        END CASE;
        CASE object_record.strategy_key
          WHEN 'binance_spot_btcusdt_l2_snapshots' THEN
            immutability_trigger := 'trg_reject_market_data_binance_spot_l2_snapshot_change';
          WHEN 'binance_spot_btcusdt_one_second_ohlcv' THEN
            immutability_trigger := 'trg_reject_market_data_binance_spot_one_second_ohlcv_change';
          WHEN 'polymarket_btc_five_minute_orderbooks' THEN
            immutability_trigger := 'trg_reject_md_polymarket_btc_five_minute_orderbook_change';
        END CASE;
        IF immutability_trigger IS NOT NULL AND NOT EXISTS (
          SELECT 1 FROM pg_trigger trigger
          WHERE trigger.tgrelid = target_relation
            AND trigger.tgname = immutability_trigger
            AND trigger.tgenabled = 'O'
        ) THEN
          RAISE EXCEPTION 'source immutability trigger is unavailable';
        END IF;
        PERFORM pg_advisory_xact_lock(hashtextextended(object_record.strategy_key, 0));
        SELECT count(*) INTO matching_chunks
        FROM timescaledb_information.chunks chunk
        WHERE chunk.hypertable_schema = target_schema
          AND chunk.hypertable_name = target_name
          AND chunk.chunk_schema = object_record.source_chunk_schema
          AND chunk.chunk_name = object_record.source_chunk_name
          AND chunk.range_start = object_record.source_start
          AND chunk.range_end = object_record.source_end;
        IF matching_chunks <> 1 THEN
          RAISE EXCEPTION 'source chunk identity or bounds changed before removal';
        END IF;

        EXECUTE format('LOCK TABLE %I.%I IN ACCESS EXCLUSIVE MODE',
          object_record.source_chunk_schema, object_record.source_chunk_name);
        EXECUTE format('SELECT count(*) FROM %s WHERE %I >= $1 AND %I < $2',
          target_relation, time_column, time_column)
          INTO current_rows USING object_record.source_start, object_record.source_end;
        IF current_rows <> object_record.row_count THEN
          RAISE EXCEPTION 'source chunk row count changed after publication: archived %, current %',
            object_record.row_count, current_rows;
        END IF;

        SELECT count(*) INTO dropped_chunks
        FROM drop_chunks(target_relation, older_than => object_record.source_end,
          newer_than => object_record.source_start);
        IF dropped_chunks <> 1 THEN
          RAISE EXCEPTION 'expected one removed chunk, removed %', dropped_chunks;
        END IF;
        UPDATE ingester.drain_objects
        SET status = 'removed', removed_at = clock_timestamp(), updated_at = clock_timestamp()
        WHERE object_id = requested_object_id;
        RETURN object_record.row_count;
      END
      $$;
      REVOKE ALL ON FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid) FROM PUBLIC;
      GRANT EXECUTE ON FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid)
        TO capitonic_ingester_worker;
    `);
  }

  public async down(_queryRunner: QueryRunner): Promise<void> {
    throw new Error('Verified source removal cannot be reversed without a separately approved migration');
  }
}
