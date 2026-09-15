import { MigrationInterface, QueryRunner } from 'typeorm';

const strategies = [
  'binance_futures_btcusdt_l2_one_second_features',
  'binance_spot_btcusdt_l2_one_second_features',
  'binance_futures_btcusdt_open_interest',
];

const cases = `
          WHEN object_record.strategy_key = 'binance_futures_btcusdt_l2_one_second_features'
            AND object_record.source_relation = 'market_data.binance_futures_btcusdt_l2_one_second_features'
          THEN target_relation := 'market_data.binance_futures_btcusdt_l2_one_second_features'::regclass;
            target_schema := 'market_data'; target_name := 'binance_futures_btcusdt_l2_one_second_features';
          WHEN object_record.strategy_key = 'binance_spot_btcusdt_l2_one_second_features'
            AND object_record.source_relation = 'market_data.binance_spot_btcusdt_l2_one_second_features'
          THEN target_relation := 'market_data.binance_spot_btcusdt_l2_one_second_features'::regclass;
            target_schema := 'market_data'; target_name := 'binance_spot_btcusdt_l2_one_second_features';
          WHEN object_record.strategy_key = 'binance_futures_btcusdt_open_interest'
            AND object_record.source_relation = 'market_data.binance_futures_btcusdt_open_interest'
          THEN target_relation := 'market_data.binance_futures_btcusdt_open_interest'::regclass;
            target_schema := 'market_data'; target_name := 'binance_futures_btcusdt_open_interest';
`;

export class RegisterBinanceFeatureDrains1789459300000
  implements MigrationInterface
{
  name = 'RegisterBinanceFeatureDrains1789459300000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      DO $$
      DECLARE definition text;
      BEGIN
        SELECT pg_get_functiondef(
          'ingester.remove_verified_drain_chunk(uuid,text)'::regprocedure
        ) INTO definition;
        definition := regexp_replace(
          definition,
          $pattern$ELSE[[:space:]]+RAISE EXCEPTION 'dataset is not registered for verified drain removal';$pattern$,
          $replacement$${cases}          ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';$replacement$
        );
        IF definition NOT LIKE '%binance_futures_btcusdt_open_interest%' THEN
          RAISE EXCEPTION 'verified drain allowlist function did not match expected definition';
        END IF;
        EXECUTE definition;
      END $$;
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      DO $$
      BEGIN
        IF EXISTS (
          SELECT 1 FROM ingester.drain_objects
          WHERE strategy_key = ANY(ARRAY[${strategies.map((key) => `'${key}'`).join(',')}])
            AND status = 'removed'
        ) THEN
          RAISE EXCEPTION 'refusing rollback after Binance feature chunks were removed';
        END IF;
      END $$;
    `);
  }
}
