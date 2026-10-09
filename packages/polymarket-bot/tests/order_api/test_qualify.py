"""Offline checks for the external driver's funded-execution boundary."""
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch

from qualify import Qualification


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
