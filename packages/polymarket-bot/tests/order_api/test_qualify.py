"""Offline checks for the external driver's funded-execution boundary."""
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch

from qualify import Qualification, REUSABLE_PROCESS_IDS, ROOT, ledger_amount


class ExecutionBoundaryTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        with patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
            self.driver = Qualification(self.directory.name)
        self.driver.state['processes'] = {
            'paper': {'process_id': 'paper-process'},
            'live': {'process_id': 'live-process'},
        }

    def test_live_execution_is_rejected_before_network_access(self):
        with patch('urllib.request.urlopen') as transport:
            with self.assertRaises(RuntimeError):
                self.driver.request('POST', '/orders/live-process/execute', {})
            transport.assert_not_called()

    def test_changed_process_mode_cannot_turn_paper_test_into_live_order(self):
        response = io.BytesIO(json.dumps({'process': {'config': {'execution': {'mode': 'live'}}}}).encode())
        response.status = 200
        with patch('urllib.request.urlopen', return_value=response) as transport:
            with self.assertRaises(RuntimeError):
                self.driver.request('POST', '/orders/paper-process/execute', {})
            self.assertEqual(transport.call_count, 1)
            self.assertEqual(transport.call_args.args[0].get_method(), 'GET')


class ProcessReuseTest(unittest.TestCase):
    def processes(self):
        return {pid: {'process_id': pid, 'enabled': False, 'status': 'stopped',
                      'config': json.loads((ROOT / f'docs/polymarket-execution-rail/examples/{mode}.json').read_text())}
                for mode, pid in REUSABLE_PROCESS_IDS.items()}

    def test_policy_comparison_reports_differences_without_mutation_or_certification(self):
        processes = self.processes()
        for difference in (False, True):
            with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
                driver = Qualification(directory)
                if difference:
                    policies = processes[REUSABLE_PROCESS_IDS['live']]['config']['execution']['order_policies']
                    next(p for p in policies if p['name'] == 'sizing_policy')['params']['default_share'] = 1
                def request(method, path):
                    self.assertEqual(method, 'GET')
                    return 200, {'process': processes[path.rsplit('/', 1)[1]]}
                with patch.object(driver, 'request', side_effect=request), patch('sys.stdout', new_callable=io.StringIO):
                    result = driver.compare_policies()
                self.assertEqual(result['shared_policies_equal'], not difference)
                self.assertEqual(result['execution_parity'], 'unverified')
                self.assertEqual(set(result['differences']), {'sizing_policy'} if difference else set())

    def test_repeated_runs_patch_same_pair_without_creating_processes(self):
        processes = self.processes()
        run_keys = []
        for order_type in ('fak', 'fok'):
            with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
                driver = Qualification(directory, order_type)
                calls = []
                def request(method, path, body=None):
                    calls.append((method, path))
                    pid = path.rsplit('/', 1)[1]
                    self.assertIn(pid, processes)
                    self.assertIn(method, ('GET', 'PATCH'))
                    if method == 'PATCH':
                        processes[pid]['config'] = body['config']
                    return 200, {'process': processes[pid]}
                with patch.object(driver, 'request', side_effect=request):
                    driver.reuse_processes()
                self.assertEqual([m for m, _ in calls], ['GET', 'GET', 'PATCH', 'PATCH'])
                self.assertEqual({m: p['process_id'] for m, p in driver.state['processes'].items()}, REUSABLE_PROCESS_IDS)
                config = driver.state['processes']['paper']['config']
                run_keys.append(config['raw']['btc_realtime_paper']['next_experiment_key'])
                policy = next(p for p in config['execution']['order_policies'] if p['name'] == 'order_execution_policy')
                self.assertEqual(policy['params']['member_order_types']['conservative'], order_type)
        self.assertNotEqual(*run_keys)

    def test_paper_only_qualification_never_reads_or_mutates_live_definition(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
            driver = Qualification(directory, mode='paper')
            process = self.processes()[REUSABLE_PROCESS_IDS['paper']]
            def request(method, path, body=None):
                self.assertTrue(path.endswith(REUSABLE_PROCESS_IDS['paper']))
                if method == 'PATCH': process['config'] = body['config']
                return 200, {'process': process}
            with patch.object(driver, 'request', side_effect=request) as api:
                driver.reuse_processes()
            self.assertEqual(list(driver.state['processes']), ['paper'])
            self.assertEqual([c.args[0] for c in api.call_args_list], ['GET', 'PATCH'])

    def test_missing_or_active_pair_never_falls_back_to_creation(self):
        for unavailable in ('missing', 'active', 'wrong_mode'):
            with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
                driver = Qualification(directory)
                processes = self.processes()
                live = processes[REUSABLE_PROCESS_IDS['live']]
                if unavailable == 'active': live['enabled'] = True
                if unavailable == 'wrong_mode': live['config']['execution']['mode'] = 'paper'
                def request(method, path):
                    pid = path.rsplit('/', 1)[1]
                    if unavailable == 'missing' and pid == live['process_id']:
                        return 404, {}
                    return 200, {'process': processes[pid]}
                with patch.object(driver, 'request', side_effect=request) as api:
                    with self.assertRaises(RuntimeError):driver.reuse_processes()
                self.assertTrue(all(c.args[0] == 'GET' for c in api.call_args_list))
                self.assertFalse(driver.state['processes'])


class SettlementEvidenceTest(unittest.TestCase):
    def test_fractional_price_improvement_uses_existing_ledger_scale(self):
        from decimal import Decimal
        debit = Decimal('0.12') * Decimal('5.4166666667') + Decimal('0.0400400000')
        self.assertEqual(ledger_amount(debit), Decimal('0.6900400000'))
        self.assertEqual(ledger_amount(-debit), Decimal('-0.6900400000'))
        self.assertEqual(ledger_amount(Decimal('5.4166666667') - debit), Decimal('4.7266266667'))
        self.assertEqual(ledger_amount(Decimal('-0.00000000005')), Decimal('-0.0000000001'))
        self.assertNotEqual(ledger_amount(debit + Decimal('0.0000000001')), Decimal('0.6900400000'))

    def test_official_winner_and_loser_credits_are_verified_without_mutation(self):
        for winner, payout, pnl in [('up', '5', '2.99'), ('down', '0', '-2.01'), ('up', '6', '3.99')]:
            with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
                driver = Qualification(directory, mode='paper')
                pid = REUSABLE_PROCESS_IDS['paper']
                driver.state['processes'] = {'paper': {'process_id': pid}}
                driver.state['executed'] = {'order_id': 'order'}
                settlement = {'credit_status': 'credited', 'filled_size': '5', 'entry_notional': '2',
                              'entry_fees': '0.01', 'payout': payout, 'net_pnl': pnl}
                receipt = {'process_id': pid, 'order_id': 'order', 'evidence': {'accounting': {'settlement': settlement}}}
                snapshot = {'orders': [{'order_id': 'order', 'raw_payload': {'request': {'market_id': 'market', 'token_id': 'up'}}}],
                            'fills': [{'order_id': 'order', 'price': '0.4', 'size': '5', 'fee': '0.01'}],
                            'settlements': [{'order_id': 'order', 'credit_status': 'credited'}]}
                with patch.object(driver, 'request', return_value=(200, receipt)) as api, patch.object(driver, 'snapshot', return_value=snapshot), patch.object(driver, 'sql', return_value={'winner': winner, 'source': 'clob_websocket'}) as sql:
                    if payout == '6':
                        with self.assertRaises(AssertionError): driver.settlement(1)
                    else:
                        driver.settlement(1)
                        self.assertEqual(driver.state['verified_settlement'], settlement)
                    api.assert_called_once_with('GET', f'/orders/{pid}/order/order')
                    self.assertTrue(sql.call_args.args[0].startswith('SELECT '))


class ContinuedQualificationTest(unittest.TestCase):
    def test_daily_loss_limit_stops_collection_without_another_submission(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
            driver = Qualification(directory, mode='paper')
            driver.state.update(processes={'paper': {'process_id': 'paper'}}, paper_dry_run={'prepared': True})
            rejection = {'status': 'rejected', 'policy_results': [{'reason': 'daily_loss_limit'}]}
            with patch.object(driver, 'candidate', return_value={'version': 1, 'candidate': {}}), patch.object(driver, 'request', return_value=(200, rejection)) as api:
                with self.assertRaisesRegex(RuntimeError, 'daily loss limit'):
                    driver.paper(1)
                self.assertEqual(api.call_count, 1)
                self.assertEqual(api.call_args.args[1], '/orders/paper/preview')
                self.assertNotIn('pending_execute', driver.state)

    def test_advance_requires_verified_unchanged_settlement_and_same_process(self):
        for invalid in ('missing', 'changed_economics', 'changed_config', 'stopped', None):
            with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
                driver = Qualification(directory, mode='paper')
                driver.state.update(processes={'paper': {'process_id': 'paper', 'config': {'policy': 1}}},
                    executed={'order_id': 'one'}, pending_execute={'idempotency_key': 'key'}, paper_after={},
                    verified_settlement={'credit_status': 'credited'}, settlement_snapshot={'credit': 5})
                if invalid == 'missing': driver.state.pop('verified_settlement')
                snapshot = {'credit': 6 if invalid == 'changed_economics' else 5}
                current = {'process': {'enabled': invalid != 'stopped', 'status': 'running',
                    'config': {'policy': 2 if invalid == 'changed_config' else 1}}}
                with patch.object(driver, 'snapshot', return_value=snapshot), patch.object(driver, 'request', return_value=(200, current)) as api:
                    if invalid:
                        with self.assertRaises((RuntimeError, AssertionError)): driver.next_order()
                        self.assertEqual(driver.state['pending_execute']['idempotency_key'], 'key')
                    else:
                        driver.next_order()
                        self.assertEqual(driver.state['completed_orders'][0]['pending_execute']['idempotency_key'], 'key')
                        self.assertEqual(driver.state['paper_before'], snapshot)
                        self.assertNotIn('pending_execute', driver.state)
                        api.assert_called_once_with('GET', '/admin/trading-processes/paper')


class LatencyEvidenceTest(unittest.TestCase):
    def test_request_duration_includes_http_error_and_transport_failure(self):
        import urllib.error
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
            driver = Qualification(directory)
            error = urllib.error.HTTPError('http://localhost', 503, 'unavailable', {}, io.BytesIO(b'{}'))
            with patch('urllib.request.urlopen', side_effect=error), patch('time.perf_counter_ns', side_effect=[100, 200000100]):
                self.assertEqual(driver.request('POST', '/orders/paper/preview', {})[0], 503)
            self.assertEqual(driver.state['requests'][-1]['duration_seconds'], .2)
            with patch('urllib.request.urlopen', side_effect=TimeoutError()), patch('time.perf_counter_ns', side_effect=[100, 300000100]):
                with self.assertRaises(TimeoutError):
                    driver.request('POST', '/orders/paper/preview', {})
            self.assertEqual(driver.state['requests'][-1]['duration_seconds'], .3)
            self.assertIsNone(driver.state['requests'][-1]['code'])

    def test_snapshot_prunes_history_at_owned_process_creation(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
            driver = Qualification(directory)
            pid = '00000000-0000-0000-0000-000000000001'
            driver.state['processes']['paper'] = {'process_id': pid, 'created_at': '2026-10-09T15:39:00Z'}
            with patch.object(driver, 'sql', return_value={}) as sql:
                driver.snapshot(pid)
            query = sql.call_args.args[0]
            self.assertEqual(query.count("timestamp_utc >= '2026-10-09T15:38:59+00:00'::timestamptz"), 2)
            self.assertIn("decision_at >= '2026-10-09T15:38:59+00:00'::timestamptz", query)
            self.assertIn("process_id='" + pid + "'", query)

    def test_sparse_samples_do_not_invent_tail_percentiles_and_replays_are_separate(self):
        from latency import report, bucket_quantile
        labels = 'process_id="p",execution_mode="paper",origin="api",stage="execution",outcome="completed"'
        lines = [f'polymarket_order_stage_duration_seconds_{suffix}{{{labels}}} {value}'
                 for suffix, value in [('count', 1), ('sum', .2)]]
        lines += [f'polymarket_order_stage_duration_seconds_bucket{{{labels},le="{bound}"}} {count}'
                  for bound, count in [(.1, 0), (.25, 1), ('+Inf', 1)]]
        lines.append(f'polymarket_order_stage_duration_max_seconds{{{labels}}} .2')
        request = {'path': '/orders/p/execute', 'code': 200, 'duration_seconds': .3, 'request': {'idempotency_key': 'k'}}
        result = report({'processes': {}, 'requests': [request, request]}, '\n'.join(lines))
        self.assertIsNone(result['stages'][0]['p95_seconds_estimate'])
        self.assertEqual(result['stages'][0]['mean_seconds'], .2)
        self.assertEqual(result['stages'][0]['max_seconds'], .2)
        self.assertEqual({r['operation'] for r in result['http']}, {'execute', 'execute_replay'})
        self.assertIsNone(bucket_quantile([(1., 10), (float('inf'), 100)], 100, .99, 100))
        self.assertAlmostEqual(bucket_quantile([(.1, 50), (.2, 100)], 100, .95, 20), .19)

if __name__ == '__main__':
    unittest.main()
