"""Offline coverage for the user-operated funded qualification boundary."""
import copy
import json
import os
import tempfile
import unittest
import uuid
from unittest.mock import patch

from paired import (PairedQualification, policies, validate_pair, signing_identity_valid,
                    require_preflight_admission, RetryablePreflight)
from qualify import ROOT, REUSABLE_PROCESS_IDS


def pair():
    result = {}
    for mode, pid in REUSABLE_PROCESS_IDS.items():
        config = json.loads((ROOT / f'docs/polymarket-execution-rail/examples/{mode}.json').read_text())
        result[mode] = {'process_id': pid, 'config': config, 'enabled': True, 'status': 'running'}
        policies(result[mode])['sizing_policy']['params'].update(default_share=5, lower_share_bound=5, upper_share_bound=5)
    return result


class PairedTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        with patch.dict(os.environ, {'POLYMARKET_HTTP_ADMIN_TOKEN': 'unused'}):
            self.q = PairedQualification(directory.name)
        self.q.state['processes'] = pair()
        self.q.state['pair'] = {'account': 'polymarket-primary', 'max_cash_usd': '5.500001',
            'requests': {}, 'attempted': [], 'order_type': 'fak', 'differences': {}}

    def requests(self):
        candidate = {'version': 1, 'candidate': {'member_id': 'conservative', 'market_id': 'market',
            'outcome': 'up', 'synthetic_prediction': {'probability_up': '0.95'}}}
        return {m: dict(candidate, idempotency_key=m + '-key') for m in REUSABLE_PROCESS_IDS}

    def test_opt_in_is_required_before_any_network_call(self):
        with patch.object(self.q, 'preflight') as preflight, patch('urllib.request.urlopen') as network:
            with self.assertRaisesRegex(RuntimeError, 'submit-live'):
                self.q.execute_pair('up', '0.95')
            with self.assertRaises(RuntimeError):
                self.q.request('POST', f"/orders/{REUSABLE_PROCESS_IDS['live']}/execute", {})
            preflight.assert_not_called()
            network.assert_not_called()

    def test_cash_bound_fixed_quantity_and_pair_contract(self):
        for mutation in ('cash', 'mode', 'account', 'quantity', 'policy', 'trigger', 'status', 'nan'):
            processes = pair()
            ceiling = '5.500001'
            live = processes['live']
            if mutation == 'cash': ceiling = '5.5'
            if mutation == 'nan': ceiling = 'NaN'
            if mutation == 'mode': live['config']['execution']['mode'] = 'paper'
            if mutation == 'account': live['config']['execution']['account_ref'] = 'other'
            if mutation == 'quantity': policies(live)['sizing_policy']['params']['default_share'] = 20
            if mutation == 'policy': policies(live)['liquidity_policy']['params']['require_exit_book'] = False
            if mutation == 'trigger': live['config']['btc_realtime_paper']['runtime']['trigger'] = 'umr'
            if mutation == 'status': live['status'] = 'stopped'
            with self.subTest(mutation=mutation), self.assertRaises(RuntimeError):
                validate_pair(processes, 'polymarket-primary', ceiling, True)
        processes = pair()
        policies(processes['paper'])['loss_limit_policy']['params']['max_daily_loss_usd'] = '25'
        result = validate_pair(processes, 'polymarket-primary', '5.500001', True)
        self.assertNotEqual(result['loss_limits']['paper'], result['loss_limits']['live'])

    def test_saved_body_and_current_config_remain_required_for_replay(self):
        self.q.submit_live = True
        bodies = self.requests()
        self.q.state['pair']['requests'] = bodies
        path = f"/orders/{REUSABLE_PROCESS_IDS['live']}/execute"
        with patch.object(self.q, 'current', return_value=pair()), patch.object(self.q, 'guard_opening_submission'):
            self.q.authorize_execution(path, bodies['live'])
            with self.assertRaises(RuntimeError):
                self.q.authorize_execution(path, dict(bodies['live'], idempotency_key='new'))
        self.q.state['pair']['attempted'] = ['live']
        changed = pair()
        policies(changed['live'])['sizing_policy']['params']['default_share'] = 20
        with patch.object(self.q, 'current', return_value=changed), self.assertRaises(RuntimeError):
            self.q.authorize_execution(path, bodies['live'])

    def test_both_identities_saved_before_post_and_timeout_never_replaced(self):
        self.q.submit_live = True
        candidate = {k: v for k, v in self.requests()['live'].items() if k != 'idempotency_key'}
        posted = []
        def post(mode):
            saved = json.loads(self.q.state_path.read_text())['pair']['requests']
            self.assertEqual(set(saved), {'paper', 'live'})
            posted.append((mode, saved[mode]['idempotency_key']))
            if mode == 'live': raise TimeoutError('unknown submission')
        with patch.object(self.q, 'wait_opening', return_value=candidate), patch.object(self.q, 'post_saved', side_effect=post):
            with self.assertRaisesRegex(RuntimeError, 'preserve evidence'):
                self.q.execute_pair('up', '0.95')
        saved = copy.deepcopy(self.q.state['pair']['requests'])
        with self.assertRaisesRegex(RuntimeError, 'Saved execution'):
            self.q.preflight('up', '0.95')
        self.assertEqual(saved, self.q.state['pair']['requests'])
        self.assertEqual({m for m, _ in posted}, {'live', 'paper'})
        self.assertNotEqual(posted[0][1], posted[1][1])

    def test_replay_uses_only_original_requests_and_detects_changed_identity(self):
        self.q.submit_live = True
        self.q.state['pair']['requests'] = self.requests()
        self.q.state['pair']['responses'] = {m: {'receipt': {'order_id': m}} for m in REUSABLE_PROCESS_IDS}
        with patch.object(self.q, 'post_saved') as post, patch.object(self.q, 'observe') as observe:
            self.q.replay()
            self.assertEqual([c.args[0] for c in post.call_args_list], ['live', 'paper'])
            observe.assert_called_once()
        def corrupt(mode):
            self.q.state['pair']['responses'][mode]['receipt']['order_id'] = 'duplicate'
        with patch.object(self.q, 'post_saved', side_effect=corrupt), self.assertRaises(AssertionError):
            self.q.replay()

    def test_live_readiness_cannot_be_inferred_from_http_success(self):
        for status in ({}, {'entries_enabled': True}):
            with patch.object(self.q, 'request', return_value=(200, {'status': {'btc_runtime': {'live_status': status}}})), self.assertRaises(AssertionError):
                self.q.live_readiness(required=True)

    def test_preflight_uses_both_modes_and_stops_before_execute_if_signing_fails(self):
        candidate = {k: v for k, v in self.requests()['live'].items() if k != 'idempotency_key'}
        for signing_ok in (True, False):
            self.q.state['checks'] = []
            self.q.state['pair']['requests'] = {}
            calls = []
            def api(method, path, body):
                calls.append((method, path, copy.deepcopy(body)))
                self.assertEqual(method, 'POST')
                self.assertNotIn('/execute', path)
                mode = 'live' if REUSABLE_PROCESS_IDS['live'] in path else 'paper'
                signing = 'performed' if mode == 'live' else 'inapplicable'
                return 200, {'status': 'evaluated', 'order_id': None, 'policy_results': [],
                    'evidence': {'proposed_order': {'token_id': 'up', 'side': 'buy', 'size': '5', 'order_type': 'fak'},
                                 'execution': {'policy_evidence': {'max_entry_debit_usd': '2.1'}}},
                    'preparation': {'serialization': 'performed', 'signing': signing,
                        'signature_type': 'Eoa', 'signer_matches_client': signing_ok, 'maker_matches_funder': True,
                        'signature_redacted': True, 'owner_redacted': True}}
            with patch.object(self.q, 'current', return_value=pair()), patch.object(self.q, 'live_readiness'), patch.object(self.q, 'candidate', return_value=copy.deepcopy(candidate)), patch.object(self.q, 'snapshot', return_value={}), patch.object(self.q, 'request', side_effect=api):
                if signing_ok:
                    result = self.q.preflight('up', '0.95')
                    self.assertEqual(result, candidate)
                    self.assertEqual(len(calls), 4)
                else:
                    with self.assertRaises(AssertionError): self.q.preflight('up', '0.95')
            self.assertFalse(self.q.state['pair']['requests'])

    def test_opening_wait_pins_market_and_discards_preparation_that_runs_late(self):
        for late in (False, True):
            clock = [100.0]
            current = pair()
            for p in current.values():
                policies(p)['entry_timing_policy']['params']['min_seconds_after_open'] = 0
            market = {'market_id': 'opening-market', 'age_seconds': 1,
                      'window_start': 'start', 'window_end': 'end'}
            def prepare(outcome, probability, candidate):
                self.assertEqual(candidate['candidate']['market_id'], 'opening-market')
                if late: clock[0] += 30
                return candidate
            with patch('paired.time.monotonic', side_effect=lambda: clock[0]), patch('paired.time.sleep'), patch.object(self.q, 'current', return_value=current), patch.object(self.q, 'opening_market', return_value=(market, 100)), patch.object(self.q, 'preflight', side_effect=prepare), patch.object(self.q, 'request') as api:
                if late:
                    with self.assertRaisesRegex(RuntimeError, 'timeout'):
                        self.q.wait_opening('up', '0.95', 10)
                else:
                    self.q.wait_opening('up', '0.95', 10)
                    self.assertEqual(self.q.state['pair']['opening_window'], market)
                api.assert_not_called()
                self.assertFalse(self.q.state['pair']['requests'])

    def test_opening_wait_skips_old_market_until_bounded_timeout(self):
        clock = [100.0]
        current = pair()
        for p in current.values():
            policies(p)['entry_timing_policy']['params']['min_seconds_after_open'] = 0
        def sleep(seconds): clock[0] += seconds
        with patch('paired.time.monotonic', side_effect=lambda: clock[0]), patch('paired.time.sleep', side_effect=sleep), patch.object(self.q, 'current', return_value=current), patch.object(self.q, 'opening_market', return_value=({'age_seconds': 26, 'market_id': 'old', 'window_end': 'next'}, 100)), patch.object(self.q, 'preflight') as prepare:
            with self.assertRaisesRegex(RuntimeError, 'timeout'):
                self.q.wait_opening('up', '0.95', 2)
            prepare.assert_not_called()
            self.assertEqual(clock[0], 102)

    def test_cutoff_blocks_new_or_ambiguous_submission_but_allows_durable_replay(self):
        body = {'idempotency_key': str(uuid.uuid4()), 'candidate': {'market_id': 'market'}}
        self.q.state['pair']['opening_window'] = {'market_id': 'market'}
        with patch('paired.time.monotonic', return_value=100):
            self.q._opening_deadline = 101
            with patch.object(self.q, 'sql') as sql:
                self.q.guard_opening_submission('live', body)
                sql.assert_not_called()
            self.q._opening_deadline = 100
            with patch.object(self.q, 'sql', return_value=False):
                with self.assertRaisesRegex(RuntimeError, 'cutoff'):
                    self.q.guard_opening_submission('live', body)
            with patch.object(self.q, 'sql', return_value=True):
                self.q.guard_opening_submission('live', body)
            with self.assertRaisesRegex(RuntimeError, 'differs'):
                self.q.guard_opening_submission('live', dict(body, candidate={'market_id': 'other'}))

    def test_setup_applies_only_matching_opening_policy_change(self):
        self.q.state['processes'] = {}
        self.q.state.pop('pair')
        current = pair()
        for p in current.values(): p.update(enabled=False, status='stopped')
        updated = []
        def api(method, path, body):
            self.assertEqual(method, 'PATCH')
            mode = next(m for m, pid in REUSABLE_PROCESS_IDS.items() if path.endswith(pid))
            result = copy.deepcopy(current[mode])
            result['config'] = body['config']
            updated.append(result)
            return 200, {'process': result}
        with patch.object(self.q, 'current', return_value=current), patch.object(self.q, 'request', side_effect=api), patch('paired.subprocess.check_output', return_value=b'{"items": []}'), patch.object(self.q, 'start'):
            self.q.setup('polymarket-primary', '5.500001', 'fak')
        self.assertEqual(len(updated), 2)
        for p in updated:
            params = policies(p)['entry_timing_policy']['params']
            self.assertEqual(params['min_seconds_after_open'], 0)
            self.assertEqual(params['min_seconds_before_close'], 90)

    def test_signing_identity_depends_on_wallet_type_and_requires_explicit_evidence(self):
        prep = {'signature_type': 'Poly1271', 'signer_matches_client': False,
                'signer_matches_funder': True, 'maker_matches_funder': True,
                'signature_redacted': True, 'owner_redacted': True}
        self.assertTrue(signing_identity_valid(prep))
        for value in (False, None):
            self.assertFalse(signing_identity_valid(dict(prep, signer_matches_funder=value)))
        old = dict(prep)
        del old['signer_matches_funder']
        with self.assertRaisesRegex(RuntimeError, 'deploy'):
            signing_identity_valid(old)
        for kind in ('Eoa', 'Proxy', 'GnosisSafe'):
            self.assertFalse(signing_identity_valid(dict(prep, signature_type=kind)))
            self.assertTrue(signing_identity_valid(dict(prep, signature_type=kind, signer_matches_client=True)))
        with self.assertRaises(RuntimeError): signing_identity_valid(dict(prep, signature_type='unknown'))

    def test_only_explicit_transient_admission_is_retryable(self):
        with self.assertRaises(RetryablePreflight): require_preflight_admission(503, {})
        with self.assertRaises(RetryablePreflight):
            require_preflight_admission(200, {'status': 'rejected', 'policy_results': [
                {'status': 'rejected', 'reason': 'price_outside_bounds'}]})
        for code in (400, 401, 403, 409, 500): require_preflight_admission(code, {})
        require_preflight_admission(200, {'status': 'rejected', 'policy_results': [
            {'status': 'rejected', 'reason': 'daily_loss_limit'}]})

    def test_opening_wait_does_not_swallow_structural_failure(self):
        current = pair()
        for p in current.values(): policies(p)['entry_timing_policy']['params']['min_seconds_after_open'] = 0
        with patch.object(self.q, 'current', return_value=current), patch.object(self.q, 'opening_market', return_value=({'market_id': 'm', 'age_seconds': 1}, 100)), patch.object(self.q, 'preflight', side_effect=AssertionError('identity mismatch')) as preflight, patch('paired.time.sleep') as sleep:
            with self.assertRaisesRegex(AssertionError, 'identity mismatch'):
                self.q.wait_opening('up', '0.95', 10)
            preflight.assert_called_once()
            sleep.assert_not_called()

    def test_opening_wait_retries_explicit_transient_then_prepares(self):
        current = pair()
        for p in current.values(): policies(p)['entry_timing_policy']['params']['min_seconds_after_open'] = 0
        market = {'market_id': 'm', 'age_seconds': 1}
        candidate = {'candidate': {'market_id': 'm'}}
        with patch.object(self.q, 'current', return_value=current), patch.object(self.q, 'opening_market', return_value=(market, 100)), patch.object(self.q, 'preflight', side_effect=[RetryablePreflight('stale book'), candidate]) as preflight, patch('paired.time.monotonic', return_value=100), patch('paired.time.sleep') as sleep:
            self.assertEqual(self.q.wait_opening('up', '0.95', 10), candidate)
            self.assertEqual(preflight.call_count, 2)
            sleep.assert_called_once()
            self.assertFalse(self.q.state['pair']['requests'])

    def test_setup_checks_both_definitions_before_any_mutation(self):
        self.q.state['processes'] = {}
        self.q.state.pop('pair')
        current = pair()
        current['paper'].update(enabled=False, status='stopped')
        with patch.object(self.q, 'current', return_value=current), patch.object(self.q, 'request') as request:
            with self.assertRaises(RuntimeError): self.q.setup('polymarket-primary', '5.500001', 'fak')
            request.assert_not_called()

    def test_observe_checks_fill_accounting_and_fok_partial_failure(self):
        for case in ('filled', 'partial_fak', 'partial_fok', 'wrong_debit', 'duplicate_credit'):
            with self.subTest(case=case):
                self.q.state['checks'] = []
                ps = self.q.state['pair']
                ps.update(requests=self.requests(), baseline={m: {'fill_debit': '0', 'settlement_credit': '0', 'decisions': 0}
                          for m in REUSABLE_PROCESS_IDS}, order_type='fok' if case == 'partial_fok' else 'fak')
                size = '2.5' if case.startswith('partial') else '5'
                debit = '1.02' if case.startswith('partial') else '2.02'
                snapshots, receipts = {}, {}
                for mode, pid in REUSABLE_PROCESS_IDS.items():
                    fill = {'fill_id': mode + '-fill', 'order_id': mode, 'price': '0.4', 'size': size,
                            'fee': '0.02', 'source': mode, 'raw_payload': {'process_id': pid}}
                    settlement = {'credit_status': 'credited', 'filled_size': size,
                        'entry_notional': '1' if case.startswith('partial') else '2', 'entry_fees': '0.02',
                        'payout': '0', 'net_pnl': '-' + debit}
                    request = {'price': '0.4', 'size': '5', 'token_id': 'up', 'market_id': 'market',
                               'process_id': pid, 'order_type': ps['order_type'],
                               'metadata': {'polymarket_execution': {'mode': mode, 'origin': 'api'}}}
                    snapshots[pid] = {'orders': [{'order_id': mode, 'client_order_id': mode + '-key',
                        'state': 'cancelled' if case.startswith('partial') else 'filled', 'raw_payload': {'request': request}}],
                        'fills': [fill], 'fill_debit': '9' if case == 'wrong_debit' else debit, 'decisions': 0,
                        'settlement_credit': '0', 'settlements': [{'order_id': mode, 'credit_status': 'credited'}]}
                    if case == 'duplicate_credit': snapshots[pid]['settlements'] *= 2
                    receipts[pid] = {'process_id': pid, 'operation_id': mode + '-key', 'order_id': mode,
                        'status': snapshots[pid]['orders'][0]['state'], 'evidence': {'fills': [fill],
                        'accounting': {'resolution': 'executed', 'settlement': settlement}}}
                def api(method, path):
                    self.assertEqual(method, 'GET')
                    pid, oid = path.split('/')[2], path.split('/')[-1]
                    return (200, receipts[pid]) if receipts[pid]['order_id'] == oid else (404, {})
                def sql(query):
                    return {'winner': 'down'} if 'official_winning_token_id' in query else False
                with patch.object(self.q, 'live_readiness', return_value={'healthy': True}), patch.object(self.q, 'snapshot', side_effect=lambda pid: snapshots[pid]), patch.object(self.q, 'request', side_effect=api), patch.object(self.q, 'sql', side_effect=sql):
                    if case in ('partial_fok', 'wrong_debit', 'duplicate_credit'):
                        with self.assertRaises(AssertionError): self.q.observe()
                    else:
                        report = self.q.observe()
                        self.assertTrue(report['modes']['live']['settlement_verified'])
                        self.assertEqual(report['live_wallet_redemption'], 'unverified')

    def test_observe_pending_is_read_only_and_never_certifies(self):
        self.q.state['pair']['requests'] = self.requests()
        with patch.object(self.q, 'live_readiness', return_value={'healthy': False}), patch.object(self.q, 'snapshot', return_value={'orders': []}), patch.object(self.q, 'request') as api:
            report = self.q.observe()
        api.assert_not_called()
        self.assertEqual(report['execution_parity'], 'pending')
        self.assertEqual(report['live_wallet_redemption'], 'unverified')


if __name__ == '__main__':
    unittest.main()
