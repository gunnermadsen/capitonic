import { MigrationInterface, QueryRunner } from 'typeorm';

export class IndexLiveRiskEvidence1790615672000
  implements MigrationInterface
{
  name = 'IndexLiveRiskEvidence1790615672000';
  transaction = false;

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_btc_settlement_live_risk_evidence
      ON polymarket.btc_paper_settlement_ledger (
        process_id,
        execution_mode,
        official_resolution_received_at DESC,
        order_id,
        settlement_id
      )
      WHERE credit_status IN ('pending', 'credited');
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      DROP INDEX CONCURRENTLY IF EXISTS polymarket.idx_btc_settlement_live_risk_evidence;
    `);
  }
}
