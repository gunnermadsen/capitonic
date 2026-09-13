import { MigrationInterface, QueryRunner } from 'typeorm';

export class AddDrainJobEvents1789459200000 implements MigrationInterface {
  name = 'AddDrainJobEvents1789459200000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE TABLE ingester.drain_job_events (
        event_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
        job_id uuid NOT NULL REFERENCES ingester.drain_jobs(job_id) ON DELETE CASCADE,
        attempt integer NOT NULL,
        recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
        level text NOT NULL,
        event_code text NOT NULL,
        message text NOT NULL,
        metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
        CONSTRAINT chk_drain_job_event_attempt CHECK (attempt >= 0),
        CONSTRAINT chk_drain_job_event_level CHECK (level IN ('info', 'warn', 'error'))
      );
      CREATE INDEX idx_drain_job_events_job_time
        ON ingester.drain_job_events (job_id, recorded_at DESC, event_id DESC);

      INSERT INTO ingester.drain_job_events (
        job_id, attempt, recorded_at, level, event_code, message, metadata
      )
      SELECT
        job_id,
        attempt,
        updated_at,
        CASE WHEN last_error_code IS NULL THEN 'info' ELSE 'error' END,
        'existing_job_snapshot',
        'drain job state captured when attempt history was enabled',
        jsonb_build_object(
          'status', status,
          'error_code', last_error_code,
          'error_message', last_error_message
        )
      FROM ingester.drain_jobs;

      CREATE FUNCTION ingester.record_drain_job_event()
      RETURNS trigger LANGUAGE plpgsql AS $$
      DECLARE
        event_level text;
        code text;
        detail text;
      BEGIN
        IF TG_OP = 'INSERT' THEN
          INSERT INTO ingester.drain_job_events (
            job_id, attempt, level, event_code, message, metadata
          ) VALUES (
            NEW.job_id, NEW.attempt, 'info', 'job_submitted', 'drain job submitted',
            jsonb_build_object('status', NEW.status, 'mode', NEW.mode)
          );
          RETURN NEW;
        END IF;

        IF NEW.status IS NOT DISTINCT FROM OLD.status
           AND NEW.attempt IS NOT DISTINCT FROM OLD.attempt
           AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code
           AND NEW.last_error_message IS NOT DISTINCT FROM OLD.last_error_message
           AND NEW.cancel_requested_at IS NOT DISTINCT FROM OLD.cancel_requested_at THEN
          RETURN NEW;
        END IF;

        IF NEW.attempt > OLD.attempt THEN
          code := 'attempt_started';
          detail := 'drain attempt started';
          event_level := 'info';
        ELSIF OLD.status = 'running'
           AND NEW.status IN ('queued', 'failed', 'cancelled')
           AND NEW.last_error_code IS NOT NULL THEN
          code := CASE WHEN NEW.status = 'queued' THEN 'attempt_requeued' ELSE 'attempt_failed' END;
          detail := COALESCE(NEW.last_error_message, 'drain attempt failed');
          event_level := 'error';
        ELSIF NEW.status = 'completed' THEN
          code := 'job_completed'; detail := 'drain job completed'; event_level := 'info';
        ELSIF NEW.status = 'cancelled' THEN
          code := 'job_cancelled'; detail := 'drain job cancelled'; event_level := 'warn';
        ELSIF NEW.status = 'queued' AND OLD.status IN ('failed', 'cancelled') THEN
          code := 'job_retried'; detail := 'drain job queued for retry'; event_level := 'info';
        ELSIF NEW.cancel_requested_at IS DISTINCT FROM OLD.cancel_requested_at THEN
          code := 'cancellation_requested'; detail := 'drain cancellation requested'; event_level := 'warn';
        ELSE
          code := 'status_changed'; detail := 'drain job status changed'; event_level := 'info';
        END IF;

        INSERT INTO ingester.drain_job_events (
          job_id, attempt, level, event_code, message, metadata
        ) VALUES (
          NEW.job_id, NEW.attempt, event_level, code, left(detail, 4000),
          jsonb_build_object(
            'previous_status', OLD.status,
            'status', NEW.status,
            'error_code', COALESCE(NEW.last_error_code, OLD.last_error_code),
            'error_message', COALESCE(NEW.last_error_message, OLD.last_error_message)
          )
        );
        RETURN NEW;
      END $$;

      CREATE TRIGGER trg_record_drain_job_event
      AFTER INSERT OR UPDATE ON ingester.drain_jobs
      FOR EACH ROW EXECUTE FUNCTION ingester.record_drain_job_event();
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      DROP TRIGGER trg_record_drain_job_event ON ingester.drain_jobs;
      DROP FUNCTION ingester.record_drain_job_event();
      DROP TABLE ingester.drain_job_events;
    `);
  }
}
