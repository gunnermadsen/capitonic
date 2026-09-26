import { MigrationInterface, QueryRunner } from 'typeorm';

export class GrantWeatherFeatureIngestion1790357000000
  implements MigrationInterface
{
  name = 'GrantWeatherFeatureIngestion1790357000000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      GRANT INSERT, UPDATE ON
        weather.goes_abi_features,
        weather.hrrr_environment_features
      TO capitonic_ingester_worker;

      GRANT SELECT, INSERT, UPDATE ON
        weather.goes_abi_window_coverage,
        weather.hrrr_environment_window_coverage
      TO capitonic_ingester_worker;
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      REVOKE INSERT, UPDATE ON
        weather.goes_abi_features,
        weather.hrrr_environment_features
      FROM capitonic_ingester_worker;

      REVOKE SELECT, INSERT, UPDATE ON
        weather.goes_abi_window_coverage,
        weather.hrrr_environment_window_coverage
      FROM capitonic_ingester_worker;
    `);
  }
}
