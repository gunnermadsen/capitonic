import { MigrationInterface, QueryRunner } from 'typeorm';

export class FixVerifiedDrainTriggerCase1790298480000
  implements MigrationInterface
{
  name = 'FixVerifiedDrainTriggerCase1790298480000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    const rows: Array<{ definition: string }> = await queryRunner.query(`
      SELECT pg_get_functiondef(
        'ingester.remove_verified_drain_chunk(uuid,text,uuid)'::regprocedure
      ) AS definition
    `);
    const definition = rows[0]?.definition;
    const triggerCase = /(WHEN 'polymarket_btc_five_minute_orderbooks' THEN\s+immutability_trigger := 'trg_reject_md_polymarket_btc_five_minute_orderbook_change';)\s+END CASE;/g;
    if (!definition || [...definition.matchAll(triggerCase)].length !== 1) {
      throw new Error('Verified drain trigger case did not match the expected function');
    }
    await queryRunner.query(
      definition.replace(
        triggerCase,
        '$1\n          ELSE immutability_trigger := NULL;\n        END CASE;',
      ),
    );
  }

  public async down(): Promise<void> {
    throw new Error('Verified drain function rollback requires a separately approved migration');
  }
}
