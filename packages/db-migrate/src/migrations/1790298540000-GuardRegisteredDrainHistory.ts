import { MigrationInterface, QueryRunner } from 'typeorm';

const addedGuards = [
  ['pmdata_chainlink_btcusd_reference_price', 'market_data', 'pmdata_chainlink_btcusd_reference_prices', 'source_timestamp'],
  ['pmdata_chainlink_btcusd_twap', 'market_data', 'pmdata_chainlink_btcusd_twap', 'source_timestamp'],
  ['polymarket_chainlink_btcusd_twap', 'market_data', 'polymarket_chainlink_btcusd_twap', 'source_timestamp'],
  ['polymarket_reference_price_ticks', 'polymarket', 'reference_price_ticks', 'source_timestamp'],
  ['chainlink_btcusd_one_minute_candles', 'market_data', 'chainlink_btcusd_one_minute_candles', 'open_timestamp'],
  ['polymarket_btc_capacity_execution_snapshots', 'polymarket', 'btc_market_capacity_execution_snapshots', 'sampled_at'],
  ['polymarket_btc_feature_snapshots', 'polymarket', 'btc_feature_snapshots', 'feature_as_of'],
  ['polygon_chainlink_btcusd_oracle_rounds', 'market_data', 'polygon_chainlink_btcusd_oracle_rounds', 'source_timestamp'],
  ['binance_futures_btcusdt_l2_one_second_features', 'market_data', 'binance_futures_btcusdt_l2_one_second_features', 'second_start'],
  ['binance_spot_btcusdt_l2_one_second_features', 'market_data', 'binance_spot_btcusdt_l2_one_second_features', 'second_start'],
  ['binance_futures_btcusdt_open_interest', 'market_data', 'binance_futures_btcusdt_open_interest', 'source_timestamp'],
] as const;

const existingGuards = [
  ['binance_spot_btcusdt_l2_snapshots', 'source_timestamp', 'trg_reject_drained_binance_spot_l2_history'],
  ['binance_spot_btcusdt_one_second_ohlcv', 'open_timestamp', 'trg_reject_drained_binance_one_second_history'],
  ['polymarket_btc_five_minute_orderbooks', 'sampled_at', 'trg_reject_drained_polymarket_orderbook_history'],
] as const;

export class GuardRegisteredDrainHistory1790298540000 implements MigrationInterface {
  name = 'GuardRegisteredDrainHistory1790298540000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    const guardCases = [
      ...existingGuards.map(([key, timestamp]) => [key, timestamp]),
      ...addedGuards.map(([key, , , timestamp]) => [key, timestamp]),
    ]
      .map(([key, timestamp]) => `WHEN '${key}' THEN row_time := NEW.${timestamp};`)
      .join('\n          ');

    await queryRunner.query(`
      CREATE OR REPLACE FUNCTION ingester.reject_drained_history_insert()
      RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
      SET search_path = pg_catalog, ingester
      AS $$
      DECLARE row_time timestamptz;
      BEGIN
        CASE TG_ARGV[0]
          ${guardCases}
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
    `);

    for (const [key, schema, table] of addedGuards) {
      await queryRunner.query(`
        CREATE TRIGGER trg_reject_drained_${key}_history
        BEFORE INSERT ON ${schema}.${table}
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_drained_history_insert('${key}');
      `);
    }

    const rows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef(
        'ingester.remove_verified_drain_chunk(uuid,text,uuid)'::regprocedure
      ) AS definition
    `);
    const definition = rows[0]?.definition;
    const declaration = 'immutability_trigger text;';
    const registrationEnd = "ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';\n        END CASE;";
    if (!definition || !definition.includes('ELSE immutability_trigger := NULL;')
      || definition.split(declaration).length !== 2
      || definition.split(registrationEnd).length !== 2) {
      throw new Error('Verified drain removal function did not match the approved baseline');
    }
    const guardNames = [
      ...existingGuards.map(([key, , trigger]) => [key, trigger]),
      ...addedGuards.map(([key]) => [key, `trg_reject_drained_${key}_history`]),
    ];
    const triggerCases = guardNames
      .map(([key, trigger]) => `WHEN '${key}' THEN guard_trigger := '${trigger}';`)
      .join('\n          ');
    const guardedDefinition = definition
      .replace(declaration, `${declaration}\n        guard_trigger text;`)
      .replace(registrationEnd, `${registrationEnd}
        CASE object_record.strategy_key
          ${triggerCases}
          ELSE RAISE EXCEPTION 'dataset has no historical insert guard';
        END CASE;
        IF NOT EXISTS (
          SELECT 1 FROM pg_trigger trigger
          WHERE trigger.tgrelid = target_relation
            AND trigger.tgname = guard_trigger
            AND trigger.tgfoid = 'ingester.reject_drained_history_insert()'::regprocedure
            AND trigger.tgenabled IN ('O', 'A')
        ) THEN
          RAISE EXCEPTION 'historical insert guard is unavailable';
        END IF;`);
    await queryRunner.query(guardedDefinition);
  }

  public async down(): Promise<void> {
    throw new Error('Verified drain safeguards require a separately approved rollback migration');
  }
}
