import { MigrationInterface, QueryRunner } from 'typeorm';

export class AuthorizeDrainWorkerChunkRemoval1789459500000
  implements MigrationInterface
{
  name = 'AuthorizeDrainWorkerChunkRemoval1789459500000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      ALTER FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid)
        SECURITY DEFINER;
      REVOKE ALL ON FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid)
        FROM PUBLIC;
      GRANT EXECUTE ON FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid)
        TO capitonic_ingester_worker;
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      REVOKE ALL ON FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid)
        FROM capitonic_ingester_worker;
      ALTER FUNCTION ingester.remove_verified_drain_chunk(uuid, text, uuid)
        SECURITY INVOKER;
    `);
  }
}
