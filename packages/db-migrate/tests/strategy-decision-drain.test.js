const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const { RegisterStrategyDecisionDrain1791043201000: Migration } = require('../dist/migrations/1791043201000-RegisterStrategyDecisionDrain');
const { IndexStrategyDecisionDrain1791043200000: Index } = require('../dist/migrations/1791043200000-IndexStrategyDecisionDrain');
const baseline = fs.readFileSync(`${__dirname}/../src/migrations/1790300000000-AddVerifiedRowDrainRemoval.ts`, 'utf8');
const fingerprint = baseline.slice(baseline.indexOf('CREATE FUNCTION ingester.verified_row_source_fingerprint'), baseline.indexOf('REVOKE ALL ON FUNCTION ingester.verified_row_source_fingerprint'));
const removal = baseline.slice(baseline.indexOf('CREATE FUNCTION ingester.remove_verified_drain_rows'), baseline.indexOf('REVOKE ALL ON FUNCTION ingester.remove_verified_drain_rows'));

async function generate(removalDefinition = removal) {
  const sql = [];
  await new Migration().up({ query: async (statement) => {
    if (statement.includes('pg_get_functiondef')) return [{ definition: statement.includes('verified_row_source_fingerprint') ? fingerprint : removalDefinition }];
    sql.push(statement);
    return [];
  } });
  return sql;
}

test('registration preserves the existing row-drain contract and protected decisions', async () => {
  const sql = await generate();
  assert.equal(sql.length, 3);
  assert.match(sql[0], /GRANT SELECT ON polymarket.btc_strategy_decisions/);
  for (const statement of sql.slice(1)) {
    assert.match(statement, /t.action = 'no_trade' AND t.status = 'rejected' AND t.order_plan_id IS NULL/);
  }
  assert.match(sql[1], /aligned five-minute windows/);
  assert.match(sql[1], /to_jsonb\(t\)::text/);
  assert.match(sql[2], /interval '12 hours'/);
  assert.match(sql[2], /FOR UPDATE/);
  assert.doesNotMatch(sql[2], /LOCK TABLE polymarket.btc_strategy_decisions/);
  assert.match(sql[2], /deleted decision contents differ from verified publication/);
  assert.match(sql[2], /job.cancel_requested_at IS NULL/);
  assert.match(sql[2], /row drain source contents changed after publication/);
  assert.match(sql[2], /affected_rows <> object_record.row_count/);
  assert.match(sql[2], /object_record.strategy_key <> 'polymarket_btc_strategy_decisions' THEN\s+GET DIAGNOSTICS/);
  assert.match(sql[2], /UPDATE polymarket.btc_interval_markets/);
});

test('unexpected baseline fails closed before removal function replacement', async () => {
  await assert.rejects(generate(removal.replace("interval '1 day'", "interval '2 days'")), /baseline did not match/);
  await assert.rejects(new Migration().down(), /separately approved/);
});

test('time index is concurrent, scoped, and checked for validity', async () => {
  const statements = [];
  const migration = new Index();
  assert.equal(migration.transaction, false);
  await migration.up({ query: async (sql) => {
    statements.push(sql);
    return [{ indisvalid: true }];
  } });
  assert.match(statements[0], /CREATE INDEX CONCURRENTLY/);
  assert.match(statements[0], /decision_at, decision_id/);
  assert.match(statements[0], /action = 'no_trade' AND status = 'rejected' AND order_plan_id IS NULL/);
  await assert.rejects(new Index().up({ query: async () => [{ indisvalid: false }] }), /not valid/);
});
