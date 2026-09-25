import { MigrationInterface, QueryRunner } from 'typeorm';

export class AllowFakEntryOrders1790353400000 implements MigrationInterface {
  name = 'AllowFakEntryOrders1790353400000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      SET LOCAL lock_timeout = '5s';
      ALTER TABLE polymarket.orders
        DROP CONSTRAINT chk_polymarket_orders_order_type,
        ADD CONSTRAINT chk_polymarket_orders_order_type
          CHECK (order_type IN ('fok', 'fak', 'gtc', 'gtd'));
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      SET LOCAL lock_timeout = '5s';
      ALTER TABLE polymarket.orders
        DROP CONSTRAINT chk_polymarket_orders_order_type,
        ADD CONSTRAINT chk_polymarket_orders_order_type
          CHECK (order_type IN ('fok', 'gtc', 'gtd'));
    `);
  }
}
