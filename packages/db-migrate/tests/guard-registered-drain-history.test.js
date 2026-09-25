const assert = require('node:assert/strict');
const { test } = require('node:test');
const { GuardRegisteredDrainHistory1790298540000 } = require('../dist/migrations/1790298540000-GuardRegisteredDrainHistory');
const { MatchDrainGuardTriggerNames1790298600000 } = require('../dist/migrations/1790298600000-MatchDrainGuardTriggerNames');

const baseline = `CREATE OR REPLACE FUNCTION ingester.remove_verified_drain_chunk()
DECLARE
        immutability_trigger text;
BEGIN
        ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';
        END CASE;
        CASE object_record.strategy_key
          ELSE immutability_trigger := NULL;
        END CASE;
END`;

async function generateSql(definition) {
  const statements = [];
  const queryRunner = {
    query: async (sql) => {
      if (sql.includes('SELECT pg_get_functiondef(')) return [{ definition }];
      statements.push(sql);
      return [];
    },
  };
  await new GuardRegisteredDrainHistory1790298540000().up(queryRunner);
  return statements;
}

test('all eleven registered tables receive matching guards and removal requires them', async () => {
  const statements = await generateSql(baseline);
  const triggers = statements.filter((sql) => sql.includes('CREATE TRIGGER'));
  assert.equal(triggers.length, 11);
  const keys = [
    'pmdata_chainlink_btcusd_reference_price',
    'pmdata_chainlink_btcusd_twap',
    'polymarket_chainlink_btcusd_twap',
    'polymarket_reference_price_ticks',
    'chainlink_btcusd_one_minute_candles',
    'polymarket_btc_capacity_execution_snapshots',
    'polymarket_btc_feature_snapshots',
    'polygon_chainlink_btcusd_oracle_rounds',
    'binance_futures_btcusdt_l2_one_second_features',
    'binance_spot_btcusdt_l2_one_second_features',
    'binance_futures_btcusdt_open_interest',
  ];
  const guardFunction = statements[0];
  const removalFunction = statements.at(-1);
  for (const key of keys) {
    assert.ok(guardFunction.includes(`WHEN '${key}' THEN row_time := NEW.`), key);
    assert.ok(triggers.some((sql) => sql.includes(`CREATE TRIGGER trg_reject_drained_${key}_history`)
      && sql.includes(`reject_drained_history_insert('${key}')`)), key);
    assert.ok(removalFunction.includes(`WHEN '${key}' THEN guard_trigger := 'trg_reject_drained_${key}_history'`), key);
  }
  assert.ok(removalFunction.includes('ELSE immutability_trigger := NULL;'));
  assert.ok(removalFunction.includes("trigger.tgfoid = 'ingester.reject_drained_history_insert()'::regprocedure"));
  assert.ok(removalFunction.includes("trigger.tgenabled IN ('O', 'A')"));
});

test('unexpected removal function baseline aborts before replacement', async () => {
  await assert.rejects(() => generateSql(baseline.replace('ELSE immutability_trigger := NULL;', '')),
    /approved baseline/);
});

test('removal checks use PostgreSQL stored trigger names for long strategies', async () => {
  const statements = await generateSql(baseline);
  const originalFunction = statements.at(-1);
  let correctedFunction;
  await new MatchDrainGuardTriggerNames1790298600000().up({
    query: async (sql) => {
      if (sql.includes('SELECT pg_get_functiondef(')) return [{ definition: originalFunction }];
      correctedFunction = sql;
      return [];
    },
  });
  const triggerNames = statements.filter((sql) => sql.includes('CREATE TRIGGER'))
    .map((sql) => sql.match(/CREATE TRIGGER (\S+)/)[1].slice(0, 63));
  for (const name of triggerNames) {
    assert.ok(correctedFunction.includes(`guard_trigger := '${name}'`), name);
  }
  assert.ok(correctedFunction.includes('ELSE immutability_trigger := NULL;'));
  assert.ok(correctedFunction.includes('historical insert guard is unavailable'));
});
