const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { test } = require('node:test');
const {
  CheckRemovedPayloadAfterConflict1790353500000,
} = require('../dist/migrations/1790353500000-CheckRemovedPayloadAfterConflict');

const projectRoot = join(__dirname, '../../..');
const originalMigration = readFileSync(join(
  projectRoot,
  'packages/db-migrate/src/migrations/1790300000000-AddVerifiedRowDrainRemoval.ts',
), 'utf8');

async function migrationSql() {
  const statements = [];
  await new CheckRemovedPayloadAfterConflict1790353500000().up({
    query: async (sql) => statements.push(sql),
  });
  assert.equal(statements.length, 1);
  return statements[0];
}

test('market conflict update checks the preserved empty payload after conflict resolution', async () => {
  const sql = await migrationSql();
  assert.match(sql, /DROP TRIGGER trg_reject_removed_market_payload_repopulation\s+ON polymarket\.btc_interval_markets/);
  assert.match(sql, /CREATE TRIGGER trg_reject_removed_market_payload_repopulation\s+AFTER INSERT OR UPDATE ON polymarket\.btc_interval_markets/);
  assert.match(originalMigration, /IF NEW\.raw_payload = '\{\}'::jsonb AND\s+NEW\.validation_errors = '\[\]'::jsonb THEN RETURN NEW/);

  for (const writer of [
    'packages/market-data-ingester/src/strategies/polymarket/backfill/support.rs',
    'packages/polymarket-bot/src/btc/repository.rs',
  ]) {
    const source = readFileSync(join(projectRoot, writer), 'utf8');
    assert.match(source, /ON CONFLICT \(market_id\) DO UPDATE SET/);
    assert.match(source, /THEN polymarket\.btc_interval_markets\.raw_payload\s+ELSE EXCLUDED\.raw_payload END/);
    assert.match(source, /THEN polymarket\.btc_interval_markets\.validation_errors\s+ELSE EXCLUDED\.validation_errors END/);
  }
});

test('reference fact conflict does nothing before the post-insert guard runs', async () => {
  const sql = await migrationSql();
  assert.match(sql, /DROP TRIGGER trg_reject_removed_reference_evidence_repopulation\s+ON polymarket\.btc_market_reference_facts/);
  assert.match(sql, /CREATE TRIGGER trg_reject_removed_reference_evidence_repopulation\s+AFTER INSERT OR UPDATE ON polymarket\.btc_market_reference_facts/);
  const writer = readFileSync(join(
    projectRoot,
    'packages/market-data-ingester/src/strategies/polymarket/backfill/support.rs',
  ), 'utf8');
  assert.match(writer, /ON CONFLICT \(market_id,fact_type,provider\) DO NOTHING/);
});

test('new and repopulating rows remain rejected while weather guards stay unchanged', async () => {
  const sql = await migrationSql();
  assert.match(sql, /SET LOCAL lock_timeout = '2s'/);
  assert.equal((sql.match(/DROP TRIGGER /g) || []).length, 2);
  assert.equal((sql.match(/CREATE TRIGGER /g) || []).length, 2);
  assert.doesNotMatch(sql, /weather\.|CREATE (?:OR REPLACE )?FUNCTION|UPDATE polymarket\.|DELETE FROM /);
  assert.match(originalMigration, /object\.status = 'removed'/);
  assert.match(originalMigration, /RAISE EXCEPTION 'historical source rows or payload have been drained'/);
  assert.match(originalMigration, /IF NEW\.evidence = '\{\}'::jsonb THEN RETURN NEW/);
  await assert.rejects(
    () => new CheckRemovedPayloadAfterConflict1790353500000().down({}),
    /separately approved migration/,
  );
});
