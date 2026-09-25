import { MigrationInterface, QueryRunner } from 'typeorm';

export class AddVerifiedRowDrainRemoval1790300000000 implements MigrationInterface {
  name = 'AddVerifiedRowDrainRemoval1790300000000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      SET LOCAL lock_timeout = '2s';
      SET LOCAL statement_timeout = '120s';
      ALTER TABLE ingester.drain_objects
        ADD COLUMN source_rows_sha256 text;
      ALTER TABLE ingester.drain_objects
        ADD CONSTRAINT chk_drain_object_source_rows_sha256 CHECK (
          source_rows_sha256 IS NULL OR source_rows_sha256 ~ '^[0-9a-f]{64}$'
        );
      ALTER TABLE ingester.drain_objects
        ADD CONSTRAINT chk_verified_row_drain_has_source_sha CHECK (
          strategy_key NOT IN (
            'goes_abi_features', 'hrrr_environment_features',
            'polymarket_btc_interval_market_payload',
            'polymarket_btc_market_reference_fact_evidence'
          ) OR status = 'staging' OR source_rows_sha256 IS NOT NULL
        );

      CREATE INDEX idx_goes_abi_features_drain_time
        ON weather.goes_abi_features (decision_time);
      CREATE INDEX idx_hrrr_environment_features_drain_time
        ON weather.hrrr_environment_features (decision_time);
      CREATE INDEX idx_btc_reference_facts_drain_time
        ON polymarket.btc_market_reference_facts (source_effective_at, fact_id);

      GRANT USAGE ON SCHEMA weather TO capitonic_ingester_worker;
      GRANT SELECT ON weather.goes_abi_features,
        weather.hrrr_environment_features TO capitonic_ingester_worker;
      GRANT USAGE ON SCHEMA ingester TO capitonic_trading;
      GRANT SELECT ON ingester.drain_objects TO capitonic_trading;
    `);

    await queryRunner.query(`
      CREATE FUNCTION ingester.verified_row_source_fingerprint(
        requested_strategy_key text, range_start timestamptz, range_end timestamptz
      ) RETURNS TABLE(row_count bigint, source_rows_sha256 text)
      LANGUAGE plpgsql SECURITY DEFINER
      SET search_path = pg_catalog, public, ingester, polymarket, weather
      AS $$
      BEGIN
        IF range_end <= range_start OR range_end > range_start + interval '1 day' THEN
          RAISE EXCEPTION 'verified row drain range must be at most one day';
        END IF;
        CASE requested_strategy_key
          WHEN 'goes_abi_features' THEN
            RETURN QUERY SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, E'\\n' ORDER BY row_json), ''), 'sha256'), 'hex')
            FROM (
              SELECT to_jsonb(t)::text AS row_json
              FROM weather.goes_abi_features t
              WHERE t.decision_time >= range_start AND t.decision_time < range_end
            ) source_rows;
          WHEN 'hrrr_environment_features' THEN
            RETURN QUERY SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, E'\\n' ORDER BY row_json), ''), 'sha256'), 'hex')
            FROM (
              SELECT to_jsonb(t)::text AS row_json
              FROM weather.hrrr_environment_features t
              WHERE t.decision_time >= range_start AND t.decision_time < range_end
            ) source_rows;
          WHEN 'polymarket_btc_interval_market_payload' THEN
            RETURN QUERY SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, E'\\n' ORDER BY row_json), ''), 'sha256'), 'hex')
            FROM (
              SELECT jsonb_build_object('market_id', t.market_id,
                'raw_payload', t.raw_payload,
                'validation_errors', t.validation_errors)::text AS row_json
              FROM polymarket.btc_interval_markets t
              WHERE t.window_start >= range_start AND t.window_start < range_end
                AND t.official_outcome IS NOT NULL
                AND NOT EXISTS (
                  SELECT 1 FROM polymarket.btc_official_resolution_watches watch
                  WHERE watch.market_id = t.market_id
                    AND watch.status IN ('pending', 'expired')
                )
                AND (t.raw_payload <> '{}'::jsonb OR
                     t.validation_errors <> '[]'::jsonb)
            ) source_rows;
          WHEN 'polymarket_btc_market_reference_fact_evidence' THEN
            RETURN QUERY SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, E'\\n' ORDER BY row_json), ''), 'sha256'), 'hex')
            FROM (
              SELECT jsonb_build_object('fact_id', t.fact_id,
                'evidence', t.evidence)::text AS row_json
              FROM polymarket.btc_market_reference_facts t
              JOIN polymarket.btc_interval_markets market
                ON market.market_id = t.market_id
              WHERE t.source_effective_at >= range_start
                AND t.source_effective_at < range_end
                AND market.official_outcome IS NOT NULL
                AND t.evidence <> '{}'::jsonb
            ) source_rows;
          ELSE
            RAISE EXCEPTION 'unregistered verified row drain strategy';
        END CASE;
      END
      $$;
      REVOKE ALL ON FUNCTION ingester.verified_row_source_fingerprint(
        text, timestamptz, timestamptz) FROM PUBLIC;
      GRANT EXECUTE ON FUNCTION ingester.verified_row_source_fingerprint(
        text, timestamptz, timestamptz) TO capitonic_ingester_worker;
    `);

    await queryRunner.query(`
      CREATE FUNCTION ingester.remove_verified_drain_rows(
        requested_object_id uuid, expected_sha256 text, requesting_job_id uuid
      ) RETURNS bigint
      LANGUAGE plpgsql SECURITY DEFINER
      SET search_path = pg_catalog, public, ingester, polymarket, weather
      AS $$
      DECLARE
        object_record ingester.drain_objects%ROWTYPE;
        job_cutoff timestamptz;
        current_count bigint;
        current_sha256 text;
        affected_rows bigint;
      BEGIN
        SELECT object.* INTO object_record
        FROM ingester.drain_objects object
        WHERE object.object_id = requested_object_id FOR UPDATE;
        IF NOT FOUND OR object_record.status <> 'published'
          OR object_record.sha256 <> expected_sha256
          OR object_record.source_rows_sha256 IS NULL
          OR object_record.source_end <> object_record.source_start + interval '1 day'
          OR object_record.source_chunk_name <> to_char(
            object_record.source_start AT TIME ZONE 'UTC', 'YYYY-MM-DD') THEN
          RAISE EXCEPTION 'row drain object is not a verified daily publication';
        END IF;
        SELECT job.cutoff INTO job_cutoff FROM ingester.drain_jobs job
        WHERE job.job_id = requesting_job_id
          AND job.strategy_key = object_record.strategy_key
          AND job.status = 'running' AND job.mode = 'drain'
          AND job.cancel_requested_at IS NULL;
        IF NOT FOUND OR object_record.source_end > job_cutoff THEN
          RAISE EXCEPTION 'row drain object is outside running drain cutoff';
        END IF;
        IF object_record.strategy_key = 'polymarket_btc_interval_market_payload'
          AND job_cutoff > clock_timestamp() - interval '5 days' THEN
          RAISE EXCEPTION 'market payload must retain five days';
        END IF;
        IF object_record.strategy_key = 'polymarket_btc_market_reference_fact_evidence'
          AND job_cutoff > clock_timestamp() - interval '3 days' THEN
          RAISE EXCEPTION 'reference evidence must retain three days';
        END IF;

        CASE object_record.strategy_key
          WHEN 'goes_abi_features' THEN
            IF object_record.source_relation <> 'weather.goes_abi_features' OR
              object_record.source_chunk_schema <> 'weather' THEN
              RAISE EXCEPTION 'GOES drain source identity mismatch';
            END IF;
            LOCK TABLE weather.goes_abi_features IN SHARE ROW EXCLUSIVE MODE;
          WHEN 'hrrr_environment_features' THEN
            IF object_record.source_relation <> 'weather.hrrr_environment_features' OR
              object_record.source_chunk_schema <> 'weather' THEN
              RAISE EXCEPTION 'HRRR drain source identity mismatch';
            END IF;
            LOCK TABLE weather.hrrr_environment_features IN SHARE ROW EXCLUSIVE MODE;
          WHEN 'polymarket_btc_interval_market_payload' THEN
            IF object_record.source_relation <> 'polymarket.btc_interval_markets' OR
              object_record.source_chunk_schema <> 'polymarket' THEN
              RAISE EXCEPTION 'market payload drain source identity mismatch';
            END IF;
            LOCK TABLE polymarket.btc_interval_markets IN SHARE ROW EXCLUSIVE MODE;
            LOCK TABLE polymarket.btc_official_resolution_watches IN SHARE MODE;
          WHEN 'polymarket_btc_market_reference_fact_evidence' THEN
            IF object_record.source_relation <> 'polymarket.btc_market_reference_facts' OR
              object_record.source_chunk_schema <> 'polymarket' THEN
              RAISE EXCEPTION 'reference evidence drain source identity mismatch';
            END IF;
            LOCK TABLE polymarket.btc_market_reference_facts IN SHARE ROW EXCLUSIVE MODE;
            LOCK TABLE polymarket.btc_interval_markets IN SHARE MODE;
          ELSE RAISE EXCEPTION 'unregistered verified row drain strategy';
        END CASE;
        SELECT fingerprint.row_count, fingerprint.source_rows_sha256
          INTO current_count, current_sha256
        FROM ingester.verified_row_source_fingerprint(
          object_record.strategy_key, object_record.source_start,
          object_record.source_end) fingerprint;
        IF current_count <> object_record.row_count
          OR current_sha256 <> object_record.source_rows_sha256 THEN
          RAISE EXCEPTION 'row drain source contents changed after publication';
        END IF;

        CASE object_record.strategy_key
          WHEN 'goes_abi_features' THEN
            DELETE FROM weather.goes_abi_features t
            WHERE t.decision_time >= object_record.source_start
              AND t.decision_time < object_record.source_end;
          WHEN 'hrrr_environment_features' THEN
            DELETE FROM weather.hrrr_environment_features t
            WHERE t.decision_time >= object_record.source_start
              AND t.decision_time < object_record.source_end;
          WHEN 'polymarket_btc_interval_market_payload' THEN
            UPDATE polymarket.btc_interval_markets t
            SET raw_payload = '{}'::jsonb,
              validation_errors = '[]'::jsonb
            WHERE t.window_start >= object_record.source_start
              AND t.window_start < object_record.source_end
              AND t.official_outcome IS NOT NULL
              AND NOT EXISTS (
                SELECT 1 FROM polymarket.btc_official_resolution_watches watch
                WHERE watch.market_id = t.market_id
                  AND watch.status IN ('pending', 'expired')
              )
              AND (t.raw_payload <> '{}'::jsonb OR
                   t.validation_errors <> '[]'::jsonb);
          WHEN 'polymarket_btc_market_reference_fact_evidence' THEN
            UPDATE polymarket.btc_market_reference_facts t
            SET evidence = '{}'::jsonb
            FROM polymarket.btc_interval_markets market
            WHERE market.market_id = t.market_id
              AND t.source_effective_at >= object_record.source_start
              AND t.source_effective_at < object_record.source_end
              AND market.official_outcome IS NOT NULL
              AND t.evidence <> '{}'::jsonb;
        END CASE;
        GET DIAGNOSTICS affected_rows = ROW_COUNT;
        IF affected_rows <> object_record.row_count THEN
          RAISE EXCEPTION 'row drain changed % rows; expected %',
            affected_rows, object_record.row_count;
        END IF;
        UPDATE ingester.drain_objects
        SET status = 'removed', removed_at = clock_timestamp(),
          updated_at = clock_timestamp()
        WHERE object_id = requested_object_id;
        RETURN affected_rows;
      END
      $$;
      REVOKE ALL ON FUNCTION ingester.remove_verified_drain_rows(uuid, text, uuid)
        FROM PUBLIC;
      GRANT EXECUTE ON FUNCTION ingester.remove_verified_drain_rows(uuid, text, uuid)
        TO capitonic_ingester_worker;
    `);

    await queryRunner.query(`
      CREATE OR REPLACE FUNCTION polymarket.reject_immutable_btc_reference_fact_change()
      RETURNS trigger LANGUAGE plpgsql AS $$
      BEGIN
        IF TG_OP = 'UPDATE' AND session_user = 'capitonic_ingester_worker'
          AND current_user <> session_user
          AND NEW.evidence = '{}'::jsonb
          AND OLD.evidence <> '{}'::jsonb
          AND (to_jsonb(NEW) - 'evidence') = (to_jsonb(OLD) - 'evidence') THEN
          RETURN NEW;
        END IF;
        RAISE EXCEPTION 'BTC market reference fact % is immutable', OLD.fact_id
          USING ERRCODE = 'integrity_constraint_violation';
      END
      $$;

      CREATE FUNCTION ingester.reject_removed_row_repopulation()
      RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
      SET search_path = pg_catalog, ingester
      AS $$
      DECLARE row_time timestamptz;
      BEGIN
        CASE TG_ARGV[0]
          WHEN 'goes_abi_features' THEN row_time := NEW.decision_time;
          WHEN 'hrrr_environment_features' THEN row_time := NEW.decision_time;
          WHEN 'polymarket_btc_interval_market_payload' THEN
            row_time := NEW.window_start;
            IF NEW.raw_payload = '{}'::jsonb AND
              NEW.validation_errors = '[]'::jsonb THEN RETURN NEW; END IF;
          WHEN 'polymarket_btc_market_reference_fact_evidence' THEN
            row_time := NEW.source_effective_at;
            IF NEW.evidence = '{}'::jsonb THEN RETURN NEW; END IF;
          ELSE RAISE EXCEPTION 'unregistered row drain insert guard target';
        END CASE;
        IF EXISTS (
          SELECT 1 FROM ingester.drain_objects object
          WHERE object.strategy_key = TG_ARGV[0]
            AND object.status = 'removed'
            AND object.source_start <= row_time
            AND object.source_end > row_time
        ) THEN
          RAISE EXCEPTION 'historical source rows or payload have been drained';
        END IF;
        RETURN NEW;
      END
      $$;
      CREATE TRIGGER trg_reject_removed_goes_feature_repopulation
        BEFORE INSERT OR UPDATE ON weather.goes_abi_features
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          'goes_abi_features');
      CREATE TRIGGER trg_reject_removed_hrrr_feature_repopulation
        BEFORE INSERT OR UPDATE ON weather.hrrr_environment_features
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          'hrrr_environment_features');
      CREATE TRIGGER trg_reject_removed_market_payload_repopulation
        BEFORE INSERT OR UPDATE ON polymarket.btc_interval_markets
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          'polymarket_btc_interval_market_payload');
      CREATE TRIGGER trg_reject_removed_reference_evidence_repopulation
        BEFORE INSERT OR UPDATE ON polymarket.btc_market_reference_facts
        FOR EACH ROW EXECUTE FUNCTION ingester.reject_removed_row_repopulation(
          'polymarket_btc_market_reference_fact_evidence');
    `);
  }

  public async down(_queryRunner: QueryRunner): Promise<void> {
    throw new Error(
      'Verified row removal cannot be reversed without a separately approved migration',
    );
  }
}
