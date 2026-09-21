import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const dashboard = JSON.parse(readFileSync(new URL(
  '../common/configs/grafana/dashboards/trading-pnl-market-metrics.json',
  import.meta.url,
)));
const panel = dashboard.panels.find(candidate => candidate.id === 27);

assert(panel, 'BTC realtime market panel must exist');
assert.equal(panel.title, 'BTC 5-Minute Realtime Market');
assert.equal(panel.type, 'trend', 'realtime market must use a numeric-x visualization');
assert.equal(panel.options.xField, 'market_elapsed_seconds');
assert.equal(panel.timeFrom, undefined, 'realtime market must not have a panel time override');
assert.equal(panel.timeShift, undefined, 'realtime market must not have a panel time shift');
assert.equal(panel.hideTimeOverride, undefined);
assert(!JSON.stringify(panel).includes('$__time'), 'realtime market must not use dashboard time macros');

const target = panel.targets.find(candidate => candidate.refId === 'A');
assert.equal(target.channel, 'stream/polymarket/btc_market_path');
assert.deepEqual(target.filter.fields, [
  'time',
  'market_elapsed_seconds',
  'twap_price',
  'price_to_beat',
]);

const elapsedOverride = panel.fieldConfig.overrides.find(
  override => override.matcher?.options === 'market_elapsed_seconds',
);
assert(elapsedOverride, 'elapsed coordinate must have an explicit field contract');

console.log('PASS realtime market panel is structurally independent of dashboard time filters');
