import { MigrationInterface, QueryRunner } from 'typeorm';

const strategy = 'binance_spot_btcusdt_aggregate_trades';
const relation = 'market_data.binance_spot_btcusdt_aggregate_trades';
const historyTrigger = 'trg_reject_drained_binance_aggregate_trade_history';

function replaceOnce(definition: string, expected: string, replacement: string): string {
  if (definition.split(expected).length !== 2) {
    throw new Error(`Verified aggregate drain baseline did not match: ${expected}`);
  }
  return definition.replace(expected, replacement);
}

export class VerifyBinanceAggregateTradeDrain1790298660000 implements MigrationInterface {
  name = 'VerifyBinanceAggregateTradeDrain1790298660000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    const guardRows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef('ingester.reject_drained_history_insert()'::regprocedure) AS definition
    `);
    const guard = guardRows[0]?.definition;
    if (!guard) throw new Error('Historical drain insert guard is missing');
    await queryRunner.query(replaceOnce(
      guard,
      "ELSE RAISE EXCEPTION 'unregistered drain insert guard target';",
      `WHEN '${strategy}' THEN row_time := NEW.trade_timestamp;\n          ELSE RAISE EXCEPTION 'unregistered drain insert guard target';`,
    ));
    await queryRunner.query(`
      CREATE TRIGGER ${historyTrigger}
      BEFORE INSERT ON ${relation}
      FOR EACH ROW EXECUTE FUNCTION ingester.reject_drained_history_insert('${strategy}');
    `);

    const removalRows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef('ingester.remove_verified_drain_chunk(uuid,text,uuid)'::regprocedure) AS definition
    `);
    let removal = removalRows[0]?.definition;
    if (!removal || !removal.includes('source chunk row count changed after publication')
      || !removal.includes('historical insert guard is unavailable')) {
      throw new Error('Verified drain removal function did not match the approved baseline');
    }
    removal = replaceOnce(
      removal,
      "ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';",
      `WHEN object_record.strategy_key = '${strategy}'
            AND object_record.source_relation = '${relation}'
          THEN target_relation := '${relation}'::regclass;
            target_schema := 'market_data'; target_name := '${strategy}';
            time_column := 'trade_timestamp';
          ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';`,
    );
    removal = replaceOnce(
      removal,
      "ELSE RAISE EXCEPTION 'dataset has no historical insert guard';",
      `WHEN '${strategy}' THEN guard_trigger := '${historyTrigger}';
          ELSE RAISE EXCEPTION 'dataset has no historical insert guard';`,
    );
    removal = replaceOnce(
      removal,
      'ELSE immutability_trigger := NULL;',
      `WHEN '${strategy}' THEN
            immutability_trigger := 'trg_reject_market_data_binance_spot_aggregate_trade_change';
          ELSE immutability_trigger := NULL;`,
    );
    await queryRunner.query(removal);

    await queryRunner.query(`
      CREATE OR REPLACE FUNCTION ingester.remove_verified_binance_aggregate_trade_chunk(
        requested_object_id uuid, expected_sha256 text
      )
      RETURNS bigint LANGUAGE plpgsql AS $$
      BEGIN
        RAISE EXCEPTION 'legacy aggregate-trade removal is disabled; use verified drain removal';
      END
      $$;
    `);
  }

  public async down(): Promise<void> {
    throw new Error('Verified source removal safeguards require a separately approved rollback migration');
  }
}
