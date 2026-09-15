require 'yaml'
require 'open3'

root = File.expand_path('..', __dir__)
path = "#{root}/common/configs/grafana/provisioning/alerting/rules-market-data-pipeline.yml"
document = YAML.respond_to?(:unsafe_load_file) ? YAML.unsafe_load_file(path) : YAML.load_file(path)
rules = document.fetch('groups').flat_map { |group| group.fetch('rules') }
source = rules.find { |rule| rule['uid'] == 'mdp_orderbook_source_no_frames' }
ingester_path = "#{root}/common/configs/grafana/provisioning/alerting/rules-market-data-ingester.yml"
ingester_document = YAML.respond_to?(:unsafe_load_file) ? YAML.unsafe_load_file(ingester_path) : YAML.load_file(ingester_path)
overflow = ingester_document.fetch('groups').flat_map { |group| group.fetch('rules') }
  .find { |rule| rule['uid'] == 'mdi_polymarket_ws_overflow' }
raise 'missing orderbook alerts' unless source && overflow
raise 'unexpected stale hold duration' unless source['for'] == '1m'
raise 'overflow must alert immediately' unless overflow['for'] == '0s'
[source, overflow].each do |rule|
  raise 'unexpected no-data behavior' unless rule['noDataState'] == 'OK'
  raise 'alert paused' if rule['isPaused']
end
expr = ->(rule) { rule.fetch('data').first.dig('model', 'expr') }
threshold = ->(rule) { rule.fetch('data').last.dig('model', 'conditions', 0, 'evaluator', 'params', 0) }
series = ->(name, values) { {'series' => name, 'values' => values} }
source_name = 'ingester_source_last_event_timestamp_seconds{product="polymarket_btc_five_minute_orderbooks",instance="owner"}'
state = 'market_data_ingester_strategy_state{strategy="polymarket_btc_five_minute_orderbooks",desired_state="running",observed_state="degraded"}'
overflow_name = 'market_data_ingester_websocket_queue_overflow_total{product="polymarket_btc_five_minute_orderbooks",instance="owner"}'
test = ->(name, rule, inputs, expected) do
  {'name' => name, 'interval' => '10s', 'input_series' => inputs,
   'promql_expr_test' => [{'expr' => "sum((#{expr.call(rule)}) > bool #{threshold.call(rule)}) or vector(0)",
     'eval_time' => '2m', 'exp_samples' => [{'labels' => '{}', 'value' => expected}]}]}
end
overload = rules.find { |rule| rule['uid'] == 'mdp_orderbook_consumer_overload' }
raise 'missing overload alert' unless overload && overload['for'] == '0s' && overload['noDataState'] == 'OK'
termination_name = 'ingester_stream_terminations_total{product="polymarket_btc_five_minute_orderbooks",instance="owner",reason="client_queue_full"}'
fixture = {'evaluation_interval'  => '10s', 'tests' => [
  test.call('no consumer overload', overload, [series.call(termination_name, '0+0x12')], 0),
  test.call('client output full', overload, [series.call(termination_name, '0+0x5 1+0x6')], 1),
  test.call('publisher broadcast lag', overload, [series.call(termination_name.sub('client_queue_full', 'broadcast_lag'), '0+0x5 1+0x6')], 1),
  test.call('client disconnect is not overload', overload, [series.call(termination_name.sub('client_queue_full', 'client_closed'), '0+1x12')], 0),
  test.call('other product overload', overload, [series.call(termination_name.sub('polymarket_btc_five_minute_orderbooks', 'other'), '0+1x12')], 0),
  test.call('overload counter reset', overload, [series.call(termination_name, '5+0x5 0+0x6')], 0),
  test.call('fresh source' , source, [series.call(source_name, '0+10x12'), series.call(state, '1+0x12')], 0),
  test.call('stalled source alerts even without subscribers', source, [series.call(source_name, '1+0x12'), series.call(state, '1+0x12')], 1),
  test.call('disabled strategy', source, [series.call(source_name, '1+0x12')], 0),
  test.call('stale former owner cannot poison healthy replacement', source, [series.call(source_name, '1+0x12'), series.call(source_name.sub('owner', 'replacement'), '0+10x12'), series.call(state, '1+0x12')], 0),
  test.call('no overflow', overflow, [series.call(overflow_name, '0+0x12')], 0),
  test.call('raw overflow', overflow, [series.call(overflow_name, '0+0x5 1+0x6')], 1),
  test.call('other product overflow', overflow, [series.call(overflow_name.sub('polymarket_btc_five_minute_orderbooks', 'other'), '0+1x12')], 0),
  test.call('counter reset is not overflow', overflow, [series.call(overflow_name, '5+0x5 0+0x6')], 0)
]}
output, status = Open3.capture2e('docker', 'exec', '-i', 'prometheus', 'promtool', 'test', 'rules', '/dev/stdin', stdin_data: YAML.dump(fixture))
puts output
abort 'Orderbook alert expression tests failed' unless status.success?
puts 'PASS orderbook source alert fixtures'
