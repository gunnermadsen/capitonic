require 'yaml'
require 'open3'

root = File.expand_path('..', __dir__)
path = "#{root}/common/configs/grafana/provisioning/alerting/rules-settlement-liveness.yml"
rules = YAML.load_file(path).fetch('groups').flat_map { |group| group.fetch('rules') }
expected = %w[
  settlement_stale_over_five_minutes
  settlement_pipeline_incomplete
  settlement_prior_run_unresolved
  settlement_autoheal_failures
]
raise 'unexpected settlement alert UIDs' unless rules.map { |rule| rule.fetch('uid') }.sort == expected.sort
raise 'duplicate settlement alert UID' unless rules.map { |rule| rule.fetch('uid') }.uniq.size == rules.size
raise 'Grafana alert UID exceeds 40 characters' unless rules.all? { |rule| rule.fetch('uid').length <= 40 }
raise 'paused settlement alert' unless rules.none? { |rule| rule.fetch('isPaused') }

entrypoint = File.read("#{root}/common/scripts/grafana-provisioning-entrypoint.sh")
source = 'rules-settlement-liveness.yml'
raise 'settlement rules are not required by provisioning' unless entrypoint.include?("Missing Grafana settlement-liveness alert provisioning source")
raise 'settlement rules are not copied by provisioning' unless entrypoint.include?("cp \"${SRC_DIR}/alerting/#{source}\"")

prometheus_rules = rules.select { |rule| rule.dig('data', 0, 'datasourceUid') == 'prometheus' }
tests = prometheus_rules.flat_map do |rule|
  expression = rule.dig('data', 0, 'model', 'expr')
  process = '{process_id="p1"}'
  healthy = case rule.fetch('uid')
            when 'settlement_stale_over_five_minutes'
              [
                {'series' => "polymarket_umr_settlement_oldest_actionable_stale_age_seconds#{process}", 'values' => '0+0x5'},
                {'series' => "polymarket_umr_settlement_actionable_stale_fills#{process}", 'values' => '0+0x5'}
              ]
            when 'settlement_pipeline_incomplete'
              %w[resolution_missing projection_missing watch_missing ledger_missing].map do |stage|
                {'series' => "polymarket_umr_settlement_#{stage}#{process}", 'values' => '0+0x5'}
              end
            else
              [{'series' => "polymarket_umr_settlement_prior_run_unresolved#{process}", 'values' => '0+0x5'}]
            end
  [{
    'name' => "#{rule.fetch('uid')} healthy fixture",
    'interval' => '1m',
    'input_series' => healthy,
    'promql_expr_test' => [{
      'expr' => "sum(#{expression}) or vector(0)",
      'eval_time' => '5m',
      'exp_samples' => [{'labels' => '{}', 'value' => 0}]
    }]
  }]
end

fixture = {'evaluation_interval' => '1m', 'tests' => tests}
output, status = Open3.capture2e(
  'docker', 'exec', '-i', 'prometheus', 'promtool', 'test', 'rules', '/dev/stdin',
  stdin_data: YAML.dump(fixture)
)
puts output
abort 'Settlement Prometheus expression tests failed' unless status.success?

loki_rule = rules.find { |rule| rule.fetch('uid') == 'settlement_autoheal_failures' }
raise 'auto-heal alert must query Loki' unless loki_rule.dig('data', 0, 'datasourceUid') == 'loki'
raise 'auto-heal alert lost its structured error selector' unless loki_rule.dig('data', 0, 'model', 'expr').include?('error_code="btc_settlement_autoheal_failed"')

puts "PASS #{rules.size} settlement alert contracts and #{tests.size} Prometheus fixtures"
