import { MigrationInterface, QueryRunner } from 'typeorm';

export class AlignTradingProcessActionPaths1792454400000 implements MigrationInterface {
  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      ALTER TABLE polymarket.trading_processes
        DROP CONSTRAINT chk_poly_trading_processes_live_account_ref;
      ALTER TABLE polymarket.trading_processes
        ADD CONSTRAINT chk_poly_trading_processes_live_account_ref CHECK (
          config #>> '{actions,0,mode}' <> 'live'
          OR (
            config #>> '{actions,0,account_ref}' IS NOT NULL
            AND config #>> '{actions,0,account_ref}' = btrim(config #>> '{actions,0,account_ref}')
            AND config #>> '{actions,0,account_ref}' ~ '^[A-Za-z0-9._:-]{1,128}$'
          )
        );
      DROP INDEX polymarket.idx_poly_trading_processes_active_live_account_ref;
      CREATE INDEX idx_poly_trading_processes_active_live_account_ref
        ON polymarket.trading_processes (lower(btrim(config #>> '{actions,0,account_ref}')), process_id)
        WHERE enabled AND status IN ('starting','running','stopping')
          AND config #>> '{actions,0,mode}' = 'live';
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      ALTER TABLE polymarket.trading_processes
        DROP CONSTRAINT chk_poly_trading_processes_live_account_ref;
      ALTER TABLE polymarket.trading_processes
        ADD CONSTRAINT chk_poly_trading_processes_live_account_ref CHECK (
          config #>> '{execution,mode}' <> 'live'
          OR (
            config #>> '{execution,account_ref}' IS NOT NULL
            AND config #>> '{execution,account_ref}' = btrim(config #>> '{execution,account_ref}')
            AND config #>> '{execution,account_ref}' ~ '^[A-Za-z0-9._:-]{1,128}$'
          )
        );
      DROP INDEX polymarket.idx_poly_trading_processes_active_live_account_ref;
      CREATE INDEX idx_poly_trading_processes_active_live_account_ref
        ON polymarket.trading_processes (lower(btrim(config #>> '{execution,account_ref}')), process_id)
        WHERE enabled AND status IN ('starting','running','stopping')
          AND config #>> '{execution,mode}' = 'live';
    `);
  }
}
