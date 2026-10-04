import { MigrationInterface, QueryRunner } from 'typeorm';

export class IndexStrategyDecisionDrain1791043200000 implements MigrationInterface {
  name = 'IndexStrategyDecisionDrain1791043200000';
  transaction = false;

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_btc_decisions_drain_time
      ON polymarket.btc_strategy_decisions (decision_at, decision_id)
      WHERE action = 'no_trade' AND status = 'rejected' AND order_plan_id IS NULL;
    `);
    const rows: Array<{ indisvalid: boolean }> = await queryRunner.query(`
      SELECT indisvalid FROM pg_index
      WHERE indexrelid = 'polymarket.idx_btc_decisions_drain_time'::regclass;
    `);
    if (!rows[0]?.indisvalid) throw new Error('Decision drain index is not valid');
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query('DROP INDEX CONCURRENTLY polymarket.idx_btc_decisions_drain_time');
  }
}
