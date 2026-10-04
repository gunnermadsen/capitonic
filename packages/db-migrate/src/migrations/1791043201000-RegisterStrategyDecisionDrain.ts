import { MigrationInterface, QueryRunner } from 'typeorm';

const key = 'polymarket_btc_strategy_decisions';
const relation = 'polymarket.btc_strategy_decisions';
const eligible = "t.action = 'no_trade' AND t.status = 'rejected' AND t.order_plan_id IS NULL";

function replaceOnce(source: string, expected: string, replacement: string): string {
  if (source.split(expected).length !== 2) {
    throw new Error(`Decision drain baseline did not match: ${expected}`);
  }
  return source.replace(expected, replacement);
}

async function definition(runner: QueryRunner, signature: string): Promise<string> {
  const rows: Array<{ definition: string }> = await runner.query(
    `SELECT pg_get_functiondef('${signature}'::regprocedure) AS definition`,
  );
  if (!rows[0]?.definition) throw new Error(`Required function is missing: ${signature}`);
  return rows[0].definition;
}

export class RegisterStrategyDecisionDrain1791043201000 implements MigrationInterface {
  name = 'RegisterStrategyDecisionDrain1791043201000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      SET LOCAL lock_timeout = '500ms';
      SET LOCAL statement_timeout = '30s';
      GRANT SELECT ON ${relation} TO capitonic_ingester_worker;
    `);
    let fingerprint = await definition(queryRunner,
      'ingester.verified_row_source_fingerprint(text,timestamptz,timestamptz)');
    fingerprint = replaceOnce(fingerprint,
      "          ELSE\n            RAISE EXCEPTION 'unregistered verified row drain strategy';",
      `          WHEN '${key}' THEN
            IF range_end <> range_start + interval '5 minutes'
              OR range_start <> date_bin('5 minutes',range_start,'1970-01-01'::timestamptz) THEN
              RAISE EXCEPTION 'decision drain requires aligned five-minute windows';
            END IF;
            RETURN QUERY SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, chr(10) ORDER BY row_json), ''), 'sha256'), 'hex')
            FROM (
              SELECT to_jsonb(t)::text AS row_json FROM ${relation} t
              WHERE t.decision_at >= range_start AND t.decision_at < range_end
                AND ${eligible}
            ) source_rows;
          ELSE
            RAISE EXCEPTION 'unregistered verified row drain strategy';`);
    await queryRunner.query(fingerprint);

    let removal = await definition(queryRunner,
      'ingester.remove_verified_drain_rows(uuid,text,uuid)');
    removal = replaceOnce(removal,
      "          OR object_record.source_end <> object_record.source_start + interval '1 day'\n          OR object_record.source_chunk_name <> to_char(\n            object_record.source_start AT TIME ZONE 'UTC', 'YYYY-MM-DD') THEN",
      `          OR (object_record.strategy_key <> '${key}' AND (
            object_record.source_end <> object_record.source_start + interval '1 day'
            OR object_record.source_chunk_name <> to_char(
              object_record.source_start AT TIME ZONE 'UTC', 'YYYY-MM-DD')))
          OR (object_record.strategy_key = '${key}' AND (
            object_record.source_end <> object_record.source_start + interval '5 minutes'
            OR object_record.source_start <> date_bin('5 minutes',
              object_record.source_start,'1970-01-01'::timestamptz)
            OR object_record.source_chunk_name <> to_char(
              object_record.source_start AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI"Z"'))) THEN`);
    removal = replaceOnce(removal,
      "        IF object_record.strategy_key = 'polymarket_btc_interval_market_payload'",
      `        IF object_record.strategy_key = '${key}' AND (
          job_cutoff > clock_timestamp() - interval '12 hours'
          OR object_record.row_count <= 0) THEN
          RAISE EXCEPTION 'decision drain must retain twelve hours and have nonempty output';
        END IF;
        IF object_record.strategy_key = 'polymarket_btc_interval_market_payload'`);
    removal = replaceOnce(removal,
      "          ELSE RAISE EXCEPTION 'unregistered verified row drain strategy';",
      `          WHEN '${key}' THEN
            IF object_record.source_relation <> '${relation}' OR
              object_record.source_chunk_schema <> 'polymarket' THEN
              RAISE EXCEPTION 'decision drain source identity mismatch';
            END IF;
            PERFORM t.decision_id FROM ${relation} t
            WHERE t.decision_at >= object_record.source_start
              AND t.decision_at < object_record.source_end AND ${eligible}
            ORDER BY t.decision_at,t.decision_id FOR UPDATE;
          ELSE RAISE EXCEPTION 'unregistered verified row drain strategy';`);
    removal = replaceOnce(removal,
      "        END CASE;\n        GET DIAGNOSTICS affected_rows = ROW_COUNT;",
      `          WHEN '${key}' THEN
            WITH removed AS (
              DELETE FROM ${relation} t
              WHERE t.decision_at >= object_record.source_start
                AND t.decision_at < object_record.source_end AND ${eligible}
              RETURNING to_jsonb(t)::text AS row_json
            ) SELECT count(*), encode(digest(
              coalesce(string_agg(row_json, chr(10) ORDER BY row_json), ''), 'sha256'), 'hex')
              INTO affected_rows,current_sha256 FROM removed;
            IF current_sha256 <> object_record.source_rows_sha256 THEN
              RAISE EXCEPTION 'deleted decision contents differ from verified publication';
            END IF;
        END CASE;
        IF object_record.strategy_key <> '${key}' THEN
          GET DIAGNOSTICS affected_rows = ROW_COUNT;
        END IF;`);
    await queryRunner.query(removal);
  }

  public async down(): Promise<void> {
    throw new Error('Decision drain removal requires a separately approved restoration migration');
  }
}
