const assert = require('node:assert/strict');
const { test } = require('node:test');
const { VerifyBinanceAggregateTradeDrain1790298660000 } = require('../dist/migrations/1790298660000-VerifyBinanceAggregateTradeDrain');

const guard = "CREATE FUNCTION guard() AS $$ ELSE RAISE EXCEPTION 'unregistered drain insert guard target'; $$";
const removal = `CREATE FUNCTION removal() AS $$
  source chunk row count changed after publication
  historical insert guard is unavailable
  ELSE RAISE EXCEPTION 'dataset is not registered for verified drain removal';
  ELSE RAISE EXCEPTION 'dataset has no historical insert guard';
  ELSE immutability_trigger := NULL;
$$`;

async function generateSql(removalDefinition = removal) {
  const statements = [];
  const runner = {
    query: async (sql) => {
      if (sql.includes("pg_get_functiondef('ingester.reject_drained_history_insert()'")) {
        return [{ definition: guard }];
      }
      if (sql.includes("pg_get_functiondef('ingester.remove_verified_drain_chunk(")) {
        return [{ definition: removalDefinition }];
      }
      statements.push(sql);
      return [];
    },
  };
  await new VerifyBinanceAggregateTradeDrain1790298660000().up(runner);
  return statements;
}

test('aggregate trades use the guarded verified chunk path', async () => {
  const statements = await generateSql();
  assert.equal(statements.length, 4);
  assert.match(statements[0], /WHEN 'binance_spot_btcusdt_aggregate_trades' THEN row_time := NEW.trade_timestamp/);
  assert.match(statements[1], /CREATE TRIGGER trg_reject_drained_binance_aggregate_trade_history/);
  assert.match(statements[2], /source_relation = 'market_data.binance_spot_btcusdt_aggregate_trades'/);
  assert.match(statements[2], /time_column := 'trade_timestamp'/);
  assert.match(statements[2], /guard_trigger := 'trg_reject_drained_binance_aggregate_trade_history'/);
  assert.match(statements[2], /immutability_trigger := 'trg_reject_market_data_binance_spot_aggregate_trade_change'/);
  assert.match(statements[3], /legacy aggregate-trade removal is disabled/);
});

test('an unexpected removal baseline prevents aggregate registration', async () => {
  await assert.rejects(
    () => generateSql(removal.replace('historical insert guard is unavailable', '')),
    /approved baseline/,
  );
});
