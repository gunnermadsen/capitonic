import { MigrationInterface, QueryRunner } from 'typeorm';

const longStrategyKeys = [
  'pmdata_chainlink_btcusd_reference_price',
  'polymarket_btc_capacity_execution_snapshots',
  'polygon_chainlink_btcusd_oracle_rounds',
  'binance_futures_btcusdt_l2_one_second_features',
  'binance_spot_btcusdt_l2_one_second_features',
  'binance_futures_btcusdt_open_interest',
] as const;

export class MatchDrainGuardTriggerNames1790298600000 implements MigrationInterface {
  name = 'MatchDrainGuardTriggerNames1790298600000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    const rows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef(
        'ingester.remove_verified_drain_chunk(uuid,text,uuid)'::regprocedure
      ) AS definition
    `);
    let definition = rows[0]?.definition;
    if (!definition || !definition.includes('historical insert guard is unavailable')) {
      throw new Error('Verified drain guard function did not match the approved baseline');
    }
    for (const key of longStrategyKeys) {
      const fullName = `trg_reject_drained_${key}_history`;
      const storedName = fullName.slice(0, 63);
      const expected = `guard_trigger := '${fullName}';`;
      if (definition.split(expected).length !== 2) {
        throw new Error(`Verified drain guard mapping did not match ${key}`);
      }
      definition = definition.replace(expected, `guard_trigger := '${storedName}';`);
    }
    await queryRunner.query(definition);
  }

  public async down(): Promise<void> {
    throw new Error('Verified drain safeguards require a separately approved rollback migration');
  }
}
