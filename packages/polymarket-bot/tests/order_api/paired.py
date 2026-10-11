"""User-operated paired order qualification through the local Capitonic order API.

No funded submission without --submit-live. Never run that option in unattended CI.
"""
import argparse
import copy
from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time
import urllib.parse
import uuid

from qualify import Qualification, REUSABLE_PROCESS_IDS, ROOT, ledger_amount


def policies(process):
    items = process['config']['execution']['order_policies']
    result = {p['name']: p for p in items}
    if len(result) != len(items):
        raise RuntimeError('Duplicate policy names')
    return result


def validate_pair(processes, account, ceiling, active):
    """Test admission bounds; execution policy evaluation stays entirely in the rail."""
    ceiling = Decimal(str(ceiling))
    if not ceiling.is_finite() or ceiling <= 0:
        raise RuntimeError('Cash ceiling must be positive and finite')
    for mode, pid in REUSABLE_PROCESS_IDS.items():
        p = processes[mode]
        e = p['config']['execution']
        if (p['process_id'] != pid or e['mode'] != mode
                or e.get('rail') != {'name': 'polymarket', 'version': 1}
                or p['config']['raw']['btc_realtime_paper']['runtime']['trigger'] != 'api'
                or not e.get('execute_signals')
                or (mode == 'live' and (not e.get('live_capital') or e.get('account_ref') != account))):
            raise RuntimeError('Retained process identity/contract/account mismatch')
        if p['enabled'] != active or p['status'] != ('running' if active else 'stopped'):
            raise RuntimeError('Both retained processes must be ' + ('running' if active else 'stopped'))
        ps = policies(p)
        sizing = ps['sizing_policy']['params']
        if sizing['mode'] != 'fixed' or any(Decimal(str(sizing[k])) != 5 for k in
                ('default_share', 'lower_share_bound', 'upper_share_bound')):
            raise RuntimeError('Paired test requires five requested shares with fixed bounds')
        # Bound a fresh execution even if its price moves after the preview.
        worst = Decimal(5) * Decimal(sizing['max_entry_price']) * (
            1 + Decimal(ps['execution_cost_policy']['params']['max_fee_rate']))
        if not worst.is_finite() or worst <= 0 or worst + Decimal('0.000001') > ceiling:
            raise RuntimeError(f'TPC worst-case cash including fee reserve and rounding exceeds {ceiling}')
    p, l = (policies(processes[m]) for m in ('paper', 'live'))
    shared = (set(p) | set(l)) - {'paper_execution_policy', 'loss_limit_policy'}
    if any(p.get(k) != l.get(k) for k in shared):
        raise RuntimeError('Shared policies differ beyond the explicitly recorded loss budget')
    loss = {m: copy.deepcopy(policies(processes[m])['loss_limit_policy']) for m in processes}
    amounts = {m: Decimal(str(loss[m]['params'].pop('max_daily_loss_usd'))) for m in processes}
    if loss['paper'] != loss['live'] or (amounts['paper'] != amounts['live']
            and amounts != {'paper': Decimal(25), 'live': Decimal(10)}):
        raise RuntimeError('Unexpected loss policy difference')
    return {'loss_limits': {m: policies(processes[m])['loss_limit_policy'] for m in processes},
            'paper_adapter': p.get('paper_execution_policy')}


class PairedQualification(Qualification):
    def __init__(self, directory, submit_live=False):
        if 'POLYMARKET_HTTP_ADMIN_TOKEN' not in os.environ:
            # Worktree .env is the symlink to the main worktree; never evaluate shell code.
            for line in (ROOT / '.env').read_text().splitlines():
                if line.startswith('POLYMARKET_HTTP_ADMIN_TOKEN='):
                    os.environ['POLYMARKET_HTTP_ADMIN_TOKEN'] = line.split('=', 1)[1].strip().strip('\"\'')
                    break
        self._save_lock = threading.RLock()
        super().__init__(directory)
        self.submit_live = submit_live

    def save(self):
        with self._save_lock:
            super().save()

    def current(self):
        result = {}
        for mode, pid in REUSABLE_PROCESS_IDS.items():
            code, response = self.request('GET', '/admin/trading-processes/' + pid)
            if code != 200:
                raise RuntimeError('Retained process unavailable: ' + mode)
            result[mode] = response['process']
        return result

    def authorize_execution(self, path, body):
        mode = next((m for m, p in REUSABLE_PROCESS_IDS.items() if path == f'/orders/{p}/execute'), None)
        pair = self.state.get('pair', {})
        if not self.submit_live or mode is None or body != pair.get('requests', {}).get(mode):
            raise RuntimeError('Paired execution requires explicit opt-in and the saved exact request')
        # Replays can become first submissions after transport failure; retain all bounds.
        current = self.current()
        validate_pair(current, pair['account'], pair['max_cash_usd'], True)
        if any(current[m]['config'] != self.state['processes'][m]['config'] for m in current):
            raise RuntimeError('TPC changed since paired preparation')
        self.guard_opening_submission(mode, body)

    def live_readiness(self, required=False):
        pid = REUSABLE_PROCESS_IDS['live']
        code, response = self.request('GET', f'/admin/trading-processes/{pid}/status')
        status = response.get('status', {}).get('btc_runtime', {}).get('live_status', {})
        healthy = (code == 200 and all(status.get(k) is True for k in
            ('entries_enabled', 'order_submit_enabled', 'user_ws_connected',
             'idempotency_clean', 'process_accounting_proven'))
            and status.get('unresolved_live_order_count') == 0
            and status.get('last_rest_reconcile_age_secs') is not None
            and 0 <= status['last_rest_reconcile_age_secs'] <= 30)
        self.state['pair']['live_readiness'] = {'healthy': healthy, 'status': status}
        self.save()
        if required:
            self.check('live process reconciliation ready', healthy, status)
        return {'healthy': healthy, 'status': status}

    def setup(self, account, ceiling, order_type):
        if self.state.get('pair') or self.state['processes']:
            raise RuntimeError('Existing run must be resumed; setup never overwrites evidence')
        current = self.current()
        proposed = copy.deepcopy(current)
        for mode, p in proposed.items():
            if p['enabled'] or p['status'] != 'stopped':
                raise RuntimeError('Stop both retained processes through the process API before setup')
            ps = policies(p)
            ps['sizing_policy']['params'].update(default_share=5, lower_share_bound=5, upper_share_bound=5)
            ps['order_execution_policy']['params']['member_order_types']['conservative'] = order_type
            ps['entry_timing_policy']['params']['min_seconds_after_open'] = 0
            run_key = 'order-api-' + mode + '-' + self.state['run']
            control = p['config']['raw']['btc_realtime_paper']
            control['next_experiment_key'] = run_key
            control['preregistration_sha256'] = hashlib.sha256(run_key.encode()).hexdigest()
        differences = validate_pair(proposed, account, ceiling, False)
        self.state['pair'] = {'account': account, 'max_cash_usd': str(ceiling),
            'order_type': order_type, 'differences': differences, 'requests': {}, 'attempted': []}
        self.state['processes_before_setup'] = current
        self.save()
        for mode, p in proposed.items():
            code, response = self.request('PATCH', '/admin/trading-processes/' + p['process_id'], {'config': p['config']})
            self.check(mode + ' configuration saved', code == 200, response if code != 200 else None)
            self.state['processes'][mode] = response['process']
            self.save()
        self.state['image'] = json.loads(subprocess.check_output(['kubectl', '--context', 'rancher-desktop',
            '-n', 'capitonic', 'get', 'pods', '-l', 'app.kubernetes.io/name=polymarket-bot', '-o', 'json']))
        self.start()

    def preflight(self, outcome, probability, candidate=None):
        pair = self.state['pair']
        if pair['requests']:
            raise RuntimeError('Saved execution exists: use observe or replay, not a new candidate')
        current = self.current()
        validate_pair(current, pair['account'], pair['max_cash_usd'], True)
        self.check('prepared configuration unchanged', all(
            current[m]['config'] == self.state['processes'][m]['config'] for m in current))
        self.live_readiness(required=True)
        candidate = copy.deepcopy(candidate) if candidate is not None else self.candidate()
        if candidate is None:
            raise RuntimeError('No current market')
        candidate['candidate'].update(outcome=outcome, synthetic_prediction={'probability_up': probability})
        snapshots = {m: self.snapshot(p['process_id']) for m, p in current.items()}
        dry_runs = {}
        for route in ('preview', 'dry-run'):
            for mode, pid in REUSABLE_PROCESS_IDS.items():
                code, receipt = self.request('POST', f'/orders/{pid}/{route}', candidate)
                self.check(mode + ' ' + route + ' admitted', code == 200 and receipt.get('status') == 'evaluated', receipt)
                self.check(mode + ' preparation has no durable identity', receipt['order_id'] is None)
                order = receipt['evidence']['proposed_order']
                self.check(mode + ' expected order', Decimal(str(order['size'])) == 5
                    and order['order_type'] == pair['order_type'] and order['side'] == 'buy')
                self.check(mode + ' prepared cash reserve within test ceiling',
                    Decimal(str(receipt['evidence']['execution']['policy_evidence']['max_entry_debit_usd']))
                    <= Decimal(pair['max_cash_usd']))
                if route == 'dry-run':
                    prep = receipt.get('preparation') or {}
                    self.check(mode + ' serialization/signing', prep.get('serialization') == 'performed'
                        and prep.get('signing') == ('performed' if mode == 'live' else 'inapplicable'))
                    if mode == 'live':
                        self.check('live signing identities match', prep.get('signer_matches_client') is True
                            and prep.get('maker_matches_funder') is True
                            and prep.get('signature_redacted') is True and prep.get('owner_redacted') is True)
                    dry_runs[mode] = receipt
        self.check('same instrument and side', all(
            dry_runs['paper']['evidence']['proposed_order'][k] == dry_runs['live']['evidence']['proposed_order'][k]
            for k in ('token_id', 'side', 'size', 'order_type')))
        decisions = {m: [p for p in dry_runs[m]['policy_results']
                          if p.get('effective_owner') in ('process_policy', 'artifact')]
                     for m in REUSABLE_PROCESS_IDS}
        self.check('shared policy decisions agree at preparation', decisions['paper'] == decisions['live'])
        for mode, pid in REUSABLE_PROCESS_IDS.items():
            self.check(mode + ' preflight has no economic effects', snapshots[mode] == self.snapshot(pid))
        pair.update(candidate=candidate, dry_runs=dry_runs, baseline=snapshots)
        self.save()
        return candidate

    def opening_market(self):
        started = time.monotonic()
        rows = self.sql("SELECT coalesce(json_agg(m),'[]') FROM (SELECT market_id,window_start,window_end, "
            "extract(epoch FROM (now()-window_start))::float8 AS age_seconds "
            "FROM polymarket.btc_interval_markets WHERE window_start <= now() "
            "AND window_start > now()-interval '5 minutes' AND window_end > now() "
            "AND window_end-window_start=interval '5 minutes' ORDER BY window_start DESC LIMIT 1) m")
        if not rows:
            return None, started
        return rows[0], started

    def wait_opening(self, outcome, probability, wait_seconds):
        if not 0 < wait_seconds <= 900:
            raise ValueError('Opening wait must be between 1 and 900 seconds')
        if self.state['pair']['requests']:
            raise RuntimeError('Saved execution exists: use observe or replay')
        current = self.current()
        validate_pair(current, self.state['pair']['account'], self.state['pair']['max_cash_usd'], True)
        if any(policies(p)['entry_timing_policy']['params']['min_seconds_after_open'] != 0
               for p in current.values()):
            raise RuntimeError('Run setup on the stopped pair to enable matching opening timing policies')
        deadline = time.monotonic() + wait_seconds
        while time.monotonic() < deadline:
            market, sampled = self.opening_market()
            if market and 0 <= market['age_seconds'] < 25:
                # Anchor to DB time and include the query round trip conservatively.
                self._opening_deadline = min(deadline, sampled + 25 - market['age_seconds'])
                candidate = {'version': 1, 'candidate': {'member_id': 'conservative',
                    'market_id': market['market_id'], 'outcome': outcome,
                    'synthetic_prediction': {'probability_up': probability}}}
                try:
                    candidate = self.preflight(outcome, probability, candidate)
                except AssertionError:
                    # Preparation alone has no submission; fresh evidence can recover in-window.
                    time.sleep(min(1, max(0, deadline - time.monotonic())))
                    continue
                if time.monotonic() < self._opening_deadline:
                    self.state['pair']['opening_window'] = market
                    self.save()
                    return candidate
            time.sleep(min(1, max(0, deadline - time.monotonic())))
        raise RuntimeError('No qualified opening window before timeout; no orders submitted')

    def guard_opening_submission(self, mode, body):
        pair = self.state['pair']
        window = pair.get('opening_window')
        if window and body['candidate']['market_id'] != window['market_id']:
            raise RuntimeError('Saved request differs from opening market')
        if window and time.monotonic() < getattr(self, '_opening_deadline', 0):
            return
        # Outside the window only an already durable operation can be replayed.
        # An attempted POST or a timeout is NOT evidence that the order exists.
        key = str(uuid.UUID(body['idempotency_key']))
        pid = REUSABLE_PROCESS_IDS[mode]
        exists = self.sql(f"SELECT EXISTS(SELECT 1 FROM polymarket.orders WHERE process_id='{pid}' AND client_order_id='{key}')")
        if not exists:
            raise RuntimeError('Opening cutoff passed; no new submission or undelivered replay allowed')

    def execute_pair(self, outcome, probability, wait_seconds=600):
        if not self.submit_live:
            raise RuntimeError('Funded execution requires --submit-live')
        candidate = self.wait_opening(outcome, probability, wait_seconds)
        pair = self.state['pair']
        pair['requests'] = {m: dict(candidate, idempotency_key=str(uuid.uuid4())) for m in REUSABLE_PROCESS_IDS}
        self.save()  # Both identities durable before either POST; never replaced after timeout.
        errors = []
        # Independent processes, same candidate; launch together without promising identical books.
        with ThreadPoolExecutor(max_workers=2) as pool:
            submitted = {m: pool.submit(self.post_saved, m) for m in ('live', 'paper')}
            for mode, future in submitted.items():
                try:
                    future.result()
                except Exception as error:
                    errors.append(mode + ': ' + str(error))
        if errors:
            raise RuntimeError('; '.join(errors) + '; preserve evidence and use observe before replay')

    def post_saved(self, mode):
        pair = self.state['pair']
        body = pair['requests'][mode]
        path = f'/orders/{REUSABLE_PROCESS_IDS[mode]}/execute'
        self.authorize_execution(path, body)
        with self._save_lock:
            pair['attempted'] = sorted(set(pair['attempted']) | {mode})
            self.save()
        code, receipt = self.request('POST', path, body)
        with self._save_lock:
            pair.setdefault('responses', {})[mode] = {'code': code, 'receipt': receipt}
            self.save()
        self.check(mode + ' durable execute response', code == 200 and receipt.get('order_id') is not None, receipt)
        self.check(mode + ' operation identity', receipt['operation_id'] == body['idempotency_key'])

    def replay(self):
        if not self.submit_live or not self.state['pair']['requests']:
            raise RuntimeError('Replay requires saved requests and --submit-live')
        # No new identities. A request that never reached the server can execute for the first time.
        original = {m: self.state['pair'].get('responses', {}).get(m, {}).get('receipt', {}).get('order_id')
                    for m in REUSABLE_PROCESS_IDS}
        for mode in ('live', 'paper'):
            self.post_saved(mode)
            if original[mode]:
                self.check(mode + ' same-key replay keeps original order',
                    self.state['pair']['responses'][mode]['receipt']['order_id'] == original[mode])
        self.state['pair']['replay_verified'] = True
        self.save()
        self.observe()

    def observe(self):
        pair = self.state['pair']
        readiness = self.live_readiness()
        report = {'live_reconciliation': readiness, 'same_key_replay_verified': pair.get('replay_verified', False),
                  'execution_parity': 'pending', 'modes': {}, 'policy_differences': pair['differences'],
                  'live_wallet_redemption': 'unverified', 'positive_credit_available_for_execution': 'unverified'}
        for mode, pid in REUSABLE_PROCESS_IDS.items():
            body = pair['requests'].get(mode)
            if not body:
                raise RuntimeError('No saved pair to observe')
            snapshot = self.snapshot(pid)
            owned = [o for o in snapshot['orders'] if o['client_order_id'] == body['idempotency_key']]
            self.check(mode + ' at most one durable order per key', len(owned) <= 1)
            if not owned:
                report['modes'][mode] = {'result': 'unknown/no durable order; do not create another key'}
                continue
            order = owned[0]
            oid = order['order_id']
            code, receipt = self.request('GET', f'/orders/{pid}/order/' + urllib.parse.quote(oid, safe=''))
            self.check(mode + ' retrieval ownership', code == 200 and receipt['process_id'] == pid
                       and receipt['operation_id'] == body['idempotency_key'] and receipt['order_id'] == oid)
            other = REUSABLE_PROCESS_IDS['paper' if mode == 'live' else 'live']
            code, _ = self.request('GET', f'/orders/{other}/order/' + urllib.parse.quote(oid, safe=''))
            self.check(mode + ' foreign retrieval denied', code == 404)
            fills = [f for f in snapshot['fills'] if f['order_id'] == oid]
            self.check(mode + ' unique canonical fills', len({f['fill_id'] for f in fills}) == len(fills)
                       and all(f['source'] == mode and f['raw_payload']['process_id'] == pid for f in fills))
            self.check(mode + ' API agrees with canonical fills', {f['fill_id'] for f in fills}
                       == {f['fill_id'] for f in receipt['evidence']['fills']})
            request = order['raw_payload']['request']
            self.check(mode + ' durable request ownership and sizing', request['process_id'] == pid
                and Decimal(str(request['size'])) == 5 and request['order_type'] == pair['order_type']
                and request['metadata']['polymarket_execution']['mode'] == mode
                and request['metadata']['polymarket_execution']['origin'] == 'api')
            notional = sum((Decimal(f['price']) * Decimal(f['size']) for f in fills), Decimal(0))
            fees = sum((Decimal(f['fee']) for f in fills), Decimal(0))
            budget = Decimal(str(request['price'])) * Decimal(str(request['size']))
            self.check(mode + ' fill prices and cash bound', all(0 < Decimal(f['price']) <= Decimal(str(request['price']))
                and Decimal(f['size']) > 0 and Decimal(f['fee']) >= 0 for f in fills)
                and notional <= budget + Decimal('0.000001') and notional + fees <= Decimal(pair['max_cash_usd']))
            if order['state'] == 'filled':
                self.check(mode + ' filled state has complete cash evidence', bool(fills)
                           and abs(notional - budget) <= Decimal('0.000001'))
            if pair['order_type'] == 'fok' and fills and order['state'] in ('filled', 'cancelled', 'expired'):
                self.check(mode + ' FOK complete cash authorization', abs(notional - budget) <= Decimal('0.000001'))
            # API-only dedicated processes must not acquire unexplained orders/debits between observations.
            baseline = pair['baseline'][mode]
            self.check(mode + ' debit matches only paired fills', Decimal(snapshot['fill_debit'])
                       - Decimal(baseline['fill_debit']) == notional + fees)
            self.check(mode + ' no UMR decisions', snapshot['decisions'] == baseline['decisions'])
            accounting = receipt['evidence'].get('accounting') or {}
            resolution = accounting.get('resolution')
            safe_oid = oid.replace("'", "''")
            reserved = self.sql(f"""SELECT EXISTS(SELECT 1 FROM polymarket.orders
                WHERE process_id='{pid}' AND order_id='{safe_oid}'
                AND (state NOT IN ('filled','rejected','cancelled','expired')
                  OR (state IN ('cancelled','expired')
                    AND raw_payload #>> '{{request,metadata,polymarket_execution,mode}}'='live'
                    AND COALESCE(raw_payload #>> '{{execution_resolution}}','')
                      NOT IN ('definitive_nonexecution','executed'))))""")
            if resolution in ('executed', 'definitive_nonexecution'):
                self.check(mode + ' durable reservation released', not reserved)
            self.check(mode + ' durable status agrees with API', receipt['status'] == order['state'])
            settlement = accounting.get('settlement')
            verified_settlement = False
            if settlement and settlement['credit_status'] == 'credited':
                market = request['market_id'].replace("'", "''")
                official = self.sql("SELECT json_build_object('winner',official_winning_token_id) FROM polymarket.btc_interval_markets WHERE market_id='" + market + "'")
                self.check(mode + ' official winner available', bool(official and official['winner']))
                quantity = sum((Decimal(f['size']) for f in fills), Decimal(0))
                payout = quantity if official['winner'] == request['token_id'] else Decimal(0)
                self.check(mode + ' settlement economics', Decimal(str(settlement['payout'])) == payout
                    and Decimal(str(settlement['net_pnl'])) == ledger_amount(payout - notional - fees)
                    and Decimal(str(settlement['filled_size'])) == quantity
                    and Decimal(str(settlement['entry_notional'])) + Decimal(str(settlement['entry_fees'])) == ledger_amount(notional + fees))
                self.check(mode + ' exactly one credited settlement', len([s for s in snapshot['settlements']
                    if s['order_id'] == oid and s['credit_status'] == 'credited']) == 1)
                self.check(mode + ' capital credit delta', Decimal(snapshot['settlement_credit'])
                    - Decimal(baseline['settlement_credit']) == payout)
                verified_settlement = True
            result = {'order_id': oid, 'status': receipt['status'], 'resolution': resolution,
                'filled': bool(fills), 'notional': str(notional), 'fees': str(fees),
                'reservation_released': not reserved,
                'settlement_verified': verified_settlement, 'settlement': settlement,
                'request': request, 'fills': fills, 'policy_results': receipt.get('policy_results'),
                'venue_acceptance_evidence': 'canonical live fills' if mode == 'live' and fills else 'not established'}
            report['modes'][mode] = result
        if all(report['modes'][m].get('filled') and report['modes'][m].get('resolution') == 'executed' for m in REUSABLE_PROCESS_IDS):
            report['execution_parity'] = 'paired executions observed; review timing/fill differences; full certification pending'
        if all(report['modes'][m].get('filled') for m in REUSABLE_PROCESS_IDS):
            totals = {}
            for mode in REUSABLE_PROCESS_IDS:
                result = report['modes'][mode]
                size = sum(Decimal(f['size']) for f in result['fills'])
                totals[mode] = {'shares': str(size), 'vwap': str(Decimal(result['notional']) / size),
                                'fees': result['fees']}
            report['fill_comparison'] = totals
            report['fill_comparison']['note'] = 'Concurrent requests observe separate books; inspect recorded timing and arrival evidence.'
        pair.setdefault('observations', []).append(report)
        self.save()
        (self.directory / 'parity.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['setup', 'preflight', 'execute', 'observe', 'replay', 'latency', 'stop'])
    parser.add_argument('--evidence-dir', required=True)
    parser.add_argument('--account', default='polymarket-primary')
    parser.add_argument('--max-cash-usd', default='5.500001')
    parser.add_argument('--order-type', choices=['fak', 'fok'], default='fak')
    parser.add_argument('--outcome', choices=['up', 'down'], default='up')
    parser.add_argument('--probability-up', default='0.95')
    parser.add_argument('--submit-live', action='store_true')
    parser.add_argument('--wait-seconds', type=int, default=600, help='Opening-window wait, 1–900 seconds; default 600')
    args = parser.parse_args()
    probability = Decimal(args.probability_up)
    if not probability.is_finite() or not 0 <= probability <= 1:
        parser.error('probability-up must be in [0,1]')
    # One local operator at a time, even across different evidence directories.
    lock_path = Path('/tmp/capitonic-retained-order-pair.lock')
    with lock_path.open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        q = PairedQualification(args.evidence_dir, args.submit_live)
        if args.action == 'setup': q.setup(args.account, args.max_cash_usd, args.order_type)
        elif args.action == 'preflight': q.preflight(args.outcome, str(probability))
        elif args.action == 'execute': q.execute_pair(args.outcome, str(probability), args.wait_seconds)
        elif args.action == 'observe': q.observe()
        elif args.action == 'replay': q.replay()
        elif args.action == 'latency': q.latency()
        elif args.action == 'stop': q.stop()


if __name__ == '__main__':
    main()
