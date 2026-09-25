import { MigrationInterface, QueryRunner } from 'typeorm';

const datasets = [
  {
    key: 'chainlink_btcusd_reference_price',
    relation: 'market_data.chainlink_btcusd_reference_prices',
    schema: 'market_data',
    table: 'chainlink_btcusd_reference_prices',
    timestamp: 'source_timestamp',
    guard: 'trg_reject_drained_chainlink_reference_history',
    immutable: 'trg_reject_market_data_chainlink_btcusd_reference_prices_change',
  },
  {
    key: 'polymarket_btc_orderbook_archive_events',
    relation: 'polymarket.btc_orderbook_archive_events',
    schema: 'polymarket',
    table: 'btc_orderbook_archive_events',
    timestamp: 'provider_received_at',
    guard: 'trg_reject_drained_btc_orderbook_archive_history',
    immutable: 'trg_reject_btc_orderbook_archive_event_change',
  },
] as const;

function replaceOnce(definition: string, expected: string, replacement: string): string {
  if (definition.split(expected).length !== 2) {
    throw new Error(`Verified drain baseline did not match: ${expected}`);
  }
  return definition.replace(expected, replacement);
}

export class RegisterChainlinkAndOrderbookArchiveDrains1790356800000 implements MigrationInterface {
  name = 'RegisterChainlinkAndOrderbookArchiveDrains1790356800000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    const guardRows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef('ingester.reject_drained_history_insert()'::regprocedure) AS definition
    `);
    let guard = guardRows[0]?.definition;
    if (!guard) throw new Error('Historical drain insert guard is missing');
    for (const dataset of datasets) {
      guard = replaceOnce(
        guard,
        "ELSE RAISE EXCEPTION 'unregistered drain insert guard target';",
        `WHEN '${dataset.key}' THEN row_time := NEW.${dataset.timestamp};
          ELSE RAISE EXCEPTION 'unregistered drain insert guard target';`,
      );
    }
    await queryRunner.query(guard);
    for (const dataset of datasets) {
      await queryRunner.query(`
        CREATE TRIGGER ${dataset.guard}
        BEFORE INSERT ON ${dataset.relation}
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_drained_history_insert('${dataset.key}');
      `);
    }

    const removalRows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef('ingester.remove_verified_drain_chunk(uuid,text,uuid)'::regprocedure) AS definition
    `);
    let removal = removalRows[0]?.definition;
    if (!removal || !removal.includes('source chunk row count changed after publication')
      || !removal.includes('historical insert guard is unavailable')) {
      throw new Error('Verified drain removal function did not match the approved baseline');
    }
    for (const dataset of datasets) {
      removal = replaceOnce(
        removal,
        "ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';",
        `WHEN object_record.strategy_key = '${dataset.key}'
            AND object_record.source_relation = '${dataset.relation}'
          THEN target_relation := '${dataset.relation}'::regclass;
            target_schema := '${dataset.schema}'; target_name := '${dataset.table}';
            time_column := '${dataset.timestamp}';
          ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';`,
      );
      removal = replaceOnce(
        removal,
        "ELSE RAISE EXCEPTION 'dataset has no historical insert guard';",
        `WHEN '${dataset.key}' THEN guard_trigger := '${dataset.guard}';
          ELSE RAISE EXCEPTION 'dataset has no historical insert guard';`,
      );
      removal = replaceOnce(
        removal,
        'ELSE immutability_trigger := NULL;',
        `WHEN '${dataset.key}' THEN immutability_trigger := '${dataset.immutable}';
          ELSE immutability_trigger := NULL;`,
      );
    }
    await queryRunner.query(removal);
  }

  public async down(): Promise<void> {
    throw new Error('Verified source removal safeguards require a separately approved rollback migration');
  }
}
