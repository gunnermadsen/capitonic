require 'yaml'
require 'open3'

root = File.expand_path('..', __dir__)
path = "#{root}/common/configs/grafana/provisioning/alerting/rules-live-execution.yml"
rules = YAML.load_file(path).fetch('groups').flat_map { |group| group.fetch('rules') }
expected = %w[
  live_daily_loss_evidence_unavailable
  live_submission_reconcile_conflict
  live_capital_evidence_unavailable
]
selected = rules.select { |rule| expected.include?(rule.fetch('uid')) }
raise 'missing live submission-risk alerts' unless selected.map { |rule| rule.fetch('uid') }.sort == expected.sort
raise 'duplicate live execution alert UID' unless rules.map { |rule| rule.fetch('uid') }.uniq.size == rules.size
raise 'Grafana alert UID exceeds 40 characters' unless rules.all? { |rule| rule.fetch('uid').length <= 40 }
raise 'paused live submission-risk alert' unless selected.none? { |rule| rule.fetch('isPaused') }

expressions = selected.to_h { |rule| [rule.fetch('uid'), rule.dig('data', 0, 'model', 'expr')] }
metric = 'polymarket_umr_live_submission_risk_checks_total'
raise 'daily-loss alert lost exact evidence reason' unless expressions.fetch('live_daily_loss_evidence_unavailable').include?("#{metric}{outcome=\"evidence_error\",reason=\"daily_loss_evidence_unavailable\"}")
raise 'capital alert lost bounded evidence reasons' unless expressions.fetch('live_capital_evidence_unavailable').include?('account_identity_evidence_unavailable|exposure_evidence_unavailable|account_order_evidence_unavailable|collateral_evidence_unavailable')
conflict = expressions.fetch('live_submission_reconcile_conflict')
raise 'reconciliation conflict alert lost submission evidence state' unless conflict.include?('1 - polymarket_umr_live_submission_risk_evidence_ready')
raise 'reconciliation conflict alert lost freshness boundary' unless conflict.include?('polymarket_umr_live_submission_risk_last_check_timestamp_seconds') && conflict.include?('< 300')
raise 'reconciliation conflict alert lost reconciliation join' unless conflict.include?('polymarket_live_reconciliation_entry_safe == 1')

process = '{process_id="p1"}'
fixture = {
  'evaluation_interval' => '1m',
  'tests' => [{
    'name' => 'submission evidence contradiction fires only on recent evidence',
    'interval' => '1m',
    'input_series' => [
      {'series' => "polymarket_umr_live_submission_risk_evidence_ready#{process}", 'values' => '0+0x5'},
      {'series' => "polymarket_umr_live_submission_risk_last_check_timestamp_seconds#{process}", 'values' => '0 60 120 180 240 300'},
      {'series' => "polymarket_live_reconciliation_entry_safe#{process}", 'values' => '1+0x5'}
    ],
    'promql_expr_test' => [{
      'expr' => conflict,
      'eval_time' => '5m',
      'exp_samples' => [{'labels' => process, 'value' => 1}]
    }]
  }]
}

output, status = Open3.capture2e(
  'docker', 'exec', '-i', 'prometheus', 'promtool', 'test', 'rules', '/dev/stdin',
  stdin_data: YAML.dump(fixture)
)
puts output
abort 'Live submission-risk Prometheus expression tests failed' unless status.success?

puts "PASS #{selected.size} live submission-risk alert contracts"
