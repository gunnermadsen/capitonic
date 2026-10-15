require 'yaml'
require 'open3'

root = File.expand_path('..', __dir__)
path = "#{root}/common/configs/grafana/provisioning/alerting/rules-market-data-pipeline.yml"
document = YAML.respond_to?(:unsafe_load_file) ? YAML.unsafe_load_file(path) : YAML.load_file(path)
rule = document.fetch('groups').flat_map { |group| group.fetch('rules') }
  .find { |candidate| candidate['uid'] == 'mdp_required_product_unready' }
raise 'missing required-product alert' unless rule
raise 'critical severity was not preserved' unless rule.dig('labels', 'severity') == 'critical'

expression = rule.fetch('data').first.dig('model', 'expr')
raise 'required-product alert must preserve the product label' unless expression.include?('by (product)')

fixture = {
  'evaluation_interval' => '10s',
  'tests' => [{
    'name' => 'required-product alerts remain product specific',
    'interval' => '10s',
    'input_series' => [
      {'series' => 'polymarket_market_data_product_ready{product="binance_futures_btcusdt_open_interest"}', 'values' => '0+0x5'},
      {'series' => 'polymarket_market_data_product_ready{product="polymarket_btc_five_minute_orderbooks"}', 'values' => '1+0x5'},
      {'series' => 'polymarket_market_data_product_ready{product="chainlink_btcusd_reference_price"}', 'values' => '0+0x5'}
    ],
    'promql_expr_test' => [{
      'expr' => expression,
      'eval_time' => '40s',
      'exp_samples' => [
        {'labels' => '{product="binance_futures_btcusdt_open_interest"}', 'value' => 1},
        {'labels' => '{product="chainlink_btcusd_reference_price"}', 'value' => 1},
        {'labels' => '{product="polymarket_btc_five_minute_orderbooks"}', 'value' => 0}
      ]
    }]
  }]
}

promtool_pod = ENV['PROMTOOL_KUBERNETES_POD']
command = if promtool_pod && !promtool_pod.empty?
            ['kubectl', '-n', 'capitonic', 'exec', '-i', promtool_pod, '--',
             'promtool', 'test', 'rules', '/dev/stdin']
          else
            ['docker', 'exec', '-i', 'prometheus', 'promtool', 'test', 'rules', '/dev/stdin']
          end
output, status = Open3.capture2e(*command, stdin_data: YAML.dump(fixture))
puts output
abort 'Required-product alert expression tests failed' unless status.success?
puts 'PASS required-product alert fixtures'
