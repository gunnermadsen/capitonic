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


if __name__ == '__main__':
    unittest.main()
