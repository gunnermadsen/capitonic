import { MigrationInterface, QueryRunner } from 'typeorm';

const datasets = [
  {
    key: 'polymarket_btc_five_minute_contract_payload',
    relation: 'market_data.polymarket_btc_five_minute_contracts',
    table: 'polymarket_btc_five_minute_contracts',
    guard: 'trg_reject_removed_five_minute_contract_payload',
    rowJson: "jsonb_build_object('market_id', t.market_id, 'revision_sha256', t.revision_sha256, 'source_payload', t.source_payload)::text",
  },
  {
    key: 'polymarket_btc_five_minute_resolution_payload',
    relation: 'market_data.polymarket_btc_five_minute_resolutions',
    table: 'polymarket_btc_five_minute_resolutions',
    guard: 'trg_reject_removed_five_minute_resolution_payload',
    rowJson: "jsonb_build_object('market_id', t.market_id, 'source', t.source, 'payload_sha256', t.payload_sha256, 'source_payload', t.source_payload)::text",
  },
] as const;

function replaceOnce(definition: string, expected: string, replacement: string): string {
  if (definition.split(expected).length !== 2) {
    throw new Error(`Verified row drain baseline did not match: ${expected}`);
  }
  return definition.replace(expected, replacement);
}

async function definition(queryRunner: QueryRunner, signature: string): Promise<string> {
  const rows: Array<{ definition: string }> = await queryRunner.query(
    `SELECT pg_get_functiondef('${signature}'::regprocedure) AS definition`,
  );
  if (!rows[0]?.definition) throw new Error(`Required function is missing: ${signature}`);
  return rows[0].definition;
}

export class RegisterFiveMinutePayloadDrains1790356900000 implements MigrationInterface {
  name = 'RegisterFiveMinutePayloadDrains1790356900000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      SET LOCAL lock_timeout = '2s';
      SET LOCAL statement_timeout = '120s';

      ALTER TABLE market_data.polymarket_btc_five_minute_contracts
        ALTER COLUMN source_payload DROP NOT NULL,
        DROP CONSTRAINT chk_market_data_polymarket_btc_five_minute_contract_payload,
        ADD CONSTRAINT chk_market_data_polymarket_btc_five_minute_contract_payload CHECK (
          (source_payload IS NULL OR (
            jsonb_typeof(source_payload) = 'object'
            AND octet_length(source_payload::text) <= 65536
            AND source_payload ->> 'version' = 'polymarket-btc-5m-contract-v1'
          ))
          AND revision_sha256 ~ '^[0-9a-f]{64}$'
          AND payload_sha256 ~ '^[0-9a-f]{64}$'
          AND revision_sha256 = payload_sha256
        );

      ALTER TABLE market_data.polymarket_btc_five_minute_resolutions
        ALTER COLUMN source_payload DROP NOT NULL,
        DROP CONSTRAINT chk_market_data_polymarket_btc_five_minute_resolution_payload,
        ADD CONSTRAINT chk_market_data_polymarket_btc_five_minute_resolution_payload CHECK (
          (source_payload IS NULL OR (
            jsonb_typeof(source_payload) = 'object'
            AND octet_length(source_payload::text) <= 1048576
          ))
          AND revision_sha256 ~ '^[0-9a-f]{64}$'
          AND payload_sha256 ~ '^[0-9a-f]{64}$'
        );

      CREATE INDEX idx_market_data_five_minute_resolution_drain_window
        ON market_data.polymarket_btc_five_minute_resolutions (window_start, market_id);

      ALTER TABLE ingester.drain_objects
        DROP CONSTRAINT chk_verified_row_drain_has_source_sha,
        ADD CONSTRAINT chk_verified_row_drain_has_source_sha CHECK (
          strategy_key NOT IN (
            'goes_abi_features', 'hrrr_environment_features',
            'polymarket_btc_interval_market_payload',
            'polymarket_btc_market_reference_fact_evidence',
            'polymarket_btc_five_minute_contract_payload',
            'polymarket_btc_five_minute_resolution_payload'
          ) OR status = 'staging' OR source_rows_sha256 IS NOT NULL
        );
    `);

    let immutable = await definition(queryRunner, 'market_data.reject_source_fact_change()');
    immutable = replaceOnce(
      immutable,
      "        RAISE EXCEPTION 'market source facts are immutable'",
      `        IF TG_OP = 'UPDATE'
          AND TG_TABLE_SCHEMA = 'market_data'
          AND TG_TABLE_NAME IN (
            'polymarket_btc_five_minute_contracts',
            'polymarket_btc_five_minute_resolutions')
          AND session_user = 'capitonic_ingester_worker'
          AND current_user <> session_user
          AND OLD.source_payload IS NOT NULL
          AND NEW.source_payload IS NULL
          AND (to_jsonb(NEW) - 'source_payload') =
              (to_jsonb(OLD) - 'source_payload') THEN
          RETURN NEW;
        END IF;
        RAISE EXCEPTION 'market source facts are immutable'`,
    );
    await queryRunner.query(immutable);

    let fingerprint = await definition(
      queryRunner,
      'ingester.verified_row_source_fingerprint(text,timestamptz,timestamptz)',
    );
    for (const dataset of datasets) {
      fingerprint = replaceOnce(
        fingerprint,
        "          ELSE\n            RAISE EXCEPTION 'unregistered verified row drain strategy';",
        `          WHEN '${dataset.key}' THEN
            RETURN QUERY SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, chr(10) ORDER BY row_json), ''),
              'sha256'), 'hex')
            FROM (
              SELECT ${dataset.rowJson} AS row_json
              FROM ${dataset.relation} t
              WHERE t.window_start >= range_start AND t.window_start < range_end
                AND t.source_payload IS NOT NULL
                AND EXISTS (
                  SELECT 1 FROM polymarket.btc_interval_markets market
                  WHERE market.market_id = t.market_id
                    AND market.official_outcome IS NOT NULL
                )
                AND NOT EXISTS (
                  SELECT 1 FROM polymarket.btc_official_resolution_watches watch
                  WHERE watch.market_id = t.market_id
                    AND watch.status IN ('pending', 'expired')
                )
            ) source_rows;
          ELSE
            RAISE EXCEPTION 'unregistered verified row drain strategy';`,
      );
    }
    await queryRunner.query(fingerprint);

    let removal = await definition(
      queryRunner,
      'ingester.remove_verified_drain_rows(uuid,text,uuid)',
    );
    removal = replaceOnce(
      removal,
      "        CASE object_record.strategy_key\n          WHEN 'goes_abi_features' THEN",
      `        IF object_record.strategy_key IN (
          'polymarket_btc_five_minute_contract_payload',
          'polymarket_btc_five_minute_resolution_payload')
          AND job_cutoff > clock_timestamp() - interval '5 days' THEN
          RAISE EXCEPTION 'five-minute source payload must retain five days';
        END IF;

        CASE object_record.strategy_key
          WHEN 'goes_abi_features' THEN`,
    );
    for (const dataset of datasets) {
      removal = replaceOnce(
        removal,
        "          ELSE RAISE EXCEPTION 'unregistered verified row drain strategy';",
        `          WHEN '${dataset.key}' THEN
            IF object_record.source_relation <> '${dataset.relation}' OR
              object_record.source_chunk_schema <> 'market_data' THEN
              RAISE EXCEPTION 'five-minute payload drain source identity mismatch';
            END IF;
            LOCK TABLE ${dataset.relation} IN SHARE ROW EXCLUSIVE MODE;
            LOCK TABLE polymarket.btc_interval_markets IN SHARE MODE;
            LOCK TABLE polymarket.btc_official_resolution_watches IN SHARE MODE;
          ELSE RAISE EXCEPTION 'unregistered verified row drain strategy';`,
      );
      removal = replaceOnce(
        removal,
        "        END CASE;\n        GET DIAGNOSTICS affected_rows = ROW_COUNT;",
        `          WHEN '${dataset.key}' THEN
            UPDATE ${dataset.relation} t
            SET source_payload = NULL
            WHERE t.window_start >= object_record.source_start
              AND t.window_start < object_record.source_end
              AND t.source_payload IS NOT NULL
              AND EXISTS (
                SELECT 1 FROM polymarket.btc_interval_markets market
                WHERE market.market_id = t.market_id
                  AND market.official_outcome IS NOT NULL
              )
              AND NOT EXISTS (
                SELECT 1 FROM polymarket.btc_official_resolution_watches watch
                WHERE watch.market_id = t.market_id
                  AND watch.status IN ('pending', 'expired')
              );
        END CASE;
        GET DIAGNOSTICS affected_rows = ROW_COUNT;`,
      );
    }
    await queryRunner.query(removal);

    let guard = await definition(queryRunner, 'ingester.reject_removed_row_repopulation()');
    for (const dataset of datasets) {
      guard = replaceOnce(
        guard,
        "          ELSE RAISE EXCEPTION 'unregistered row drain insert guard target';",
        `          WHEN '${dataset.key}' THEN
            row_time := NEW.window_start;
            IF TG_OP = 'UPDATE' AND OLD.source_payload IS NOT NULL
              AND NEW.source_payload IS NULL THEN RETURN NEW; END IF;
            IF NEW.source_payload IS NULL THEN
              RAISE EXCEPTION 'five-minute payload requires verified drain removal';
            END IF;
          ELSE RAISE EXCEPTION 'unregistered row drain insert guard target';`,
      );
    }
    await queryRunner.query(guard);
    for (const dataset of datasets) {
      await queryRunner.query(`
        CREATE TRIGGER ${dataset.guard}
        AFTER INSERT OR UPDATE ON ${dataset.relation}
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          '${dataset.key}');
      `);
    }
  }

  public async down(): Promise<void> {
    throw new Error(
      'Verified payload removal cannot be reversed without a separately approved migration',
    );
  }
}
