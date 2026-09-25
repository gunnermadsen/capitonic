import { MigrationInterface, QueryRunner } from 'typeorm';

export class CheckRemovedPayloadAfterConflict1790353500000 implements MigrationInterface {
  name = 'CheckRemovedPayloadAfterConflict1790353500000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      SET LOCAL lock_timeout = '2s';

      DROP TRIGGER trg_reject_removed_market_payload_repopulation
        ON polymarket.btc_interval_markets;
      CREATE TRIGGER trg_reject_removed_market_payload_repopulation
        AFTER INSERT OR UPDATE ON polymarket.btc_interval_markets
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          'polymarket_btc_interval_market_payload');

      DROP TRIGGER trg_reject_removed_reference_evidence_repopulation
        ON polymarket.btc_market_reference_facts;
      CREATE TRIGGER trg_reject_removed_reference_evidence_repopulation
        AFTER INSERT OR UPDATE ON polymarket.btc_market_reference_facts
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          'polymarket_btc_market_reference_fact_evidence');
    `);
  }

  public async down(_queryRunner: QueryRunner): Promise<void> {
    throw new Error(
      'Restoring pre-conflict payload guards requires a separately approved migration',
    );
  }
}
