import { MigrationInterface, QueryRunner } from 'typeorm';

/** Registers a stream-only strategy; creates no market-data storage. */
export class RegisterKrakenSpotTradeStream1789603200000 implements MigrationInterface {
  name = 'RegisterKrakenSpotTradeStream1789603200000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      INSERT INTO ingester.profiles
        (strategy_key, config_schema_version, config, desired_state, observed_state, health_status)
      VALUES ('kraken_spot_btcusd_trades', 1, '{}'::jsonb, 'stopped', 'stopped', 'unknown')
      ON CONFLICT (strategy_key) DO NOTHING
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    // Preserve control history and the profile identity for safe image rollback.
    await queryRunner.query(`
      UPDATE ingester.profiles
      SET desired_state = 'stopped', desired_generation = desired_generation + 1, updated_at = now()
      WHERE strategy_key = 'kraken_spot_btcusd_trades' AND desired_state <> 'stopped'
    `);
  }
}
