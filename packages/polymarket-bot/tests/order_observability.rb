# Validate the committed order queries against an existing promtool, without provisioning.
# Usage: ruby packages/polymarket-bot/tests/order_observability.rb <command ... promtool>
require 'yaml'
require 'json'
require 'open3'
root = File.expand_path('../../..', __dir__)
path = "#{root}/common/configs/grafana/provisioning/alerting/rules-execution-freshness.yml"
groups = YAML.load_file(path).fetch('groups')
rules = groups.flat_map { |g| g.fetch('rules') }
all = Dir["#{root}/common/configs/grafana/provisioning/alerting/*.yml"].flat_map { |p| YAML.load_file(p).fetch('groups', []).flat_map { |g| g.fetch('rules') } }
raise 'duplicate alert UID' unless all.map { |r| r['uid'] }.uniq.length == all.length
selected = %w[order_execution_recovery_unhealthy order_execution_errors].to_h { |uid| [uid, rules.find { |r| r['uid'] == uid } || raise("missing #{uid}")] }
selected.each_value { |r| raise 'incorrect idle/error behavior' unless r['noDataState']=='OK' && r['execErrState']=='Error' && !r['isPaused'] }
entry = File.read("#{root}/common/scripts/grafana-provisioning-entrypoint.sh")
raise 'alert source is not provisioned' unless entry.include?('cp "${SRC_DIR}/alerting/rules-execution-freshness.yml"')
dashboard = JSON.parse(File.read("#{root}/common/configs/grafana/dashboards/order-execution.json"))
raise 'Loki JSON fields not extracted' unless dashboard['panels'].select { |p| p['type']=='logs' }.all? { |p| p['targets'].all? { |t| t['expr'].include?('fields.process_id') } }
raise 'economic origin missing' unless dashboard['panels'].find { |p| p['type']=='table' }['targets'][0]['rawSql'].include?('polymarket_execution,origin')
expr = ->(uid) { selected.fetch(uid)['data'][0]['model']['expr'] }
series = ->(name, labels, values) { {'series'=>"#{name}{#{labels}}", 'values'=>values} }
a='process_id="a",execution_mode="paper",instance="bot",job="polymarket-bot"'
b='process_id="b",execution_mode="live",instance="bot",job="polymarket-bot"'
fresh=[series.call('polymarket_order_last_recovery_timestamp_seconds',a,'0+60x10'),series.call('polymarket_order_last_recovery_timestamp_seconds',b,'0+60x10')]
health=[series.call('polymarket_order_recovery_healthy',a,'0+0x10'),series.call('polymarket_order_recovery_healthy',b,'1+0x10')]
tests=[]
add=->(name,input,expression,expected){tests << {'name'=>name,'interval'=>'1m','input_series'=>input,'promql_expr_test'=>[{'expr'=>expression,'eval_time'=>'5m','exp_samples'=>expected.map { |labels,value| {'labels'=>labels,'value'=>value} }}]}}
add.call('healthy peer cannot mask paper failure',fresh+health,expr.call('order_execution_recovery_unhealthy'),[['{execution_mode="paper",process_id="a"}',1],['{execution_mode="live",process_id="b"}',0]])
add.call('stale recovery cannot be hidden by previously healthy result',[series.call('polymarket_order_recovery_healthy',a,'1+0x10'),series.call('polymarket_order_last_recovery_timestamp_seconds',a,'0+0x10')],expr.call('order_execution_recovery_unhealthy'),[['{execution_mode="paper",process_id="a"}',1]])
add.call('inactive new execution is not an outage',[],expr.call('order_execution_recovery_unhealthy'),[])
add.call('errors retain process mode and origin',[series.call('polymarket_order_stage_events_total',a+',origin="api",stage="submission",outcome="error"','0+1x10'),series.call('polymarket_order_stage_events_total',b+',origin="umr",stage="submission",outcome="error"','0+0x10')],expr.call('order_execution_errors'),[['{execution_mode="paper",origin="api",process_id="a"}',5],['{execution_mode="live",origin="umr",process_id="b"}',0]])
dashboard['panels'].select { |p| p.dig('datasource','uid')=='prometheus' }.each do |panel|
  panel['targets'].each do |target|
    query=target['expr'].gsub('$__rate_interval','5m').gsub('$process_id','a').gsub('$execution_mode','.*').gsub('$origin','.*')
    add.call("dashboard expression #{panel['id']}/#{target['refId']}",[],query,[])
  end
end
abort 'Pass an existing promtool command; this test never starts or deploys a service.' if ARGV.empty?
output,status=Open3.capture2e(*ARGV,'test','rules','/dev/stdin',stdin_data:YAML.dump({'evaluation_interval'=>'1m','tests'=>tests}))
puts output
abort 'order observability query tests failed' unless status.success?
puts "PASS #{tests.length} order query fixtures, JSON extraction, unique alerts and existing provisioning ownership"
