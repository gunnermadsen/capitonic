"""Opt-in HTTP qualification of the deployed order application; never submits live orders.

Uses normal process/order APIs and bounded read-only SQL against local development.
No test implementation, feature flag, or persistence path is installed in the rail.
"""
import argparse
import copy
from datetime import datetime, timezone, timedelta
from decimal import Decimal, ROUND_HALF_UP
import hashlib
import json
import os
from latency import report as latency_report
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[4]
# Local qualification owns exactly this pair across runs; never create replacements.
REUSABLE_PROCESS_IDS = {
    'paper': 'b4171f11-9fde-45e0-ad8c-37f6954769ec',
    'live': 'bd63f9ce-e58b-4395-aee2-aab3ce197621',
}


def ledger_amount(value):
    """Existing settlement NUMERIC(30,10), including PostgreSQL tie rounding."""
    return value.quantize(Decimal('0.0000000001'), rounding=ROUND_HALF_UP)


def process_configuration(process):
    return {key: process[key] for key in ('execution', 'sources', 'btc_realtime_paper')}


class Qualification:
    def __init__(self, directory, order_type="fak", mode="both"):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.state_path = self.directory / 'run.json'
        self.state = json.loads(self.state_path.read_text()) if self.state_path.exists() else {
            'run': str(uuid.uuid4()), 'processes': {}, 'checks': [], 'requests': []}
        self.state.setdefault('order_type', order_type)
        self.state.setdefault('modes', ['paper', 'live'] if mode == 'both' else ['paper'])
        self.token = os.environ['POLYMARKET_HTTP_ADMIN_TOKEN']
        self.base = 'http://localhost/api/bot'

    def save(self):
        temporary = self.state_path.with_suffix('.tmp')
        with temporary.open('w') as output:
            output.write(json.dumps(self.state, indent=2) + '\n')
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(self.state_path)

    def authorize_execution(self, path, body):
        pid = path.split('/')[2]
        if pid != self.state['processes']['paper']['process_id']:
            raise RuntimeError('Only the owned paper process may execute')
        code, actual = self.request('GET', '/admin/trading-processes/' + pid)
        if code != 200 or actual['process']['execution']['mode'] != 'paper':
            raise RuntimeError('Current process must be paper before execution')

    def check(self, name, condition, detail=None):
        self.state['checks'].append({'name': name, 'passed': bool(condition), 'detail': detail})
        self.save()
        print(('PASS ' if condition else 'FAIL ') + name, flush=True)
        if not condition:
            raise AssertionError(f'{name}: {detail}')

    def request(self, method, path, body=None, auth=True):
        # Enforce the funded-test boundary independently of request construction.
        if path.endswith('/execute'):
            self.authorize_execution(path, body)
        headers = {'Content-Type': 'application/json'}
        if auth:
            headers['Authorization'] = 'Bearer ' + self.token
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, headers=headers, method=method)
        started = time.perf_counter_ns()
        try:
            with urllib.request.urlopen(req, timeout=45) as r:
                code, raw = r.status, r.read().decode()
        except urllib.error.HTTPError as e:
            code, raw = e.code, e.read().decode()
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            self.state['requests'].append({'at': datetime.now(timezone.utc).isoformat(),
                'method': method, 'path': path, 'request': body, 'code': None,
                'duration_seconds': (time.perf_counter_ns() - started) / 1e9,
                'response': {'error_type': type(error).__name__}})
            self.save()
            raise
        elapsed = (time.perf_counter_ns() - started) / 1e9
        try:
            result = json.loads(raw)
        except ValueError:
            result = {'text': raw}
        self.state['requests'].append({'at': datetime.now(timezone.utc).isoformat(),
            'method': method, 'path': path, 'request': body, 'code': code, 'response': result, 'duration_seconds': elapsed})
        self.save()
        return code, result

    def sql(self, query):
        if not query.lstrip().upper().startswith('SELECT '):
            raise ValueError('Qualification SQL must be read-only')
        result = subprocess.run(['kubectl', '--context', 'rancher-desktop', '-n', 'capitonic',
            'exec', 'timescaledb-0', '-c', 'timescaledb', '--', 'env',
            'PGOPTIONS=-c default_transaction_read_only=on -c statement_timeout=5000',
            'psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres', '-d', 'polymarket',
            '-Atc', query], capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return json.loads(result.stdout)

    def snapshot(self, pid):
        pid = str(uuid.UUID(pid))
        owned = next(p for p in self.state['processes'].values() if p['process_id'] == pid)
        # Process ownership starts at creation, including all previous test runs. Prune historical
        # Timescale chunks without changing the audited economic scope or timeout.
        since = (datetime.fromisoformat(owned['created_at'].replace('Z', '+00:00')) - timedelta(seconds=1)).isoformat()
        # Canonical manifests cover every owned run; run_id leads the decision index.
        return self.sql(f"""SELECT json_build_object(
          'orders',(SELECT coalesce(json_agg(x ORDER BY x.order_id),'[]') FROM
            (SELECT order_id,client_order_id,state,raw_payload FROM polymarket.orders WHERE process_id='{pid}' ORDER BY created_at DESC, order_id DESC LIMIT 30) x),
          'fills',(SELECT coalesce(json_agg(x ORDER BY x.fill_id),'[]') FROM
            (SELECT fill_id,order_id,price::text,size::text,fee::text,source,raw_payload FROM polymarket.fills WHERE timestamp_utc >= '{since}'::timestamptz AND process_id='{pid}' ORDER BY timestamp_utc DESC, fill_id DESC LIMIT 30) x),
          'settlements',(SELECT coalesce(json_agg(x ORDER BY x.order_id),'[]') FROM
            (SELECT order_id,payout::text,net_pnl::text,credit_status FROM polymarket.btc_paper_settlement_ledger WHERE process_id='{pid}' ORDER BY created_at DESC, order_id DESC LIMIT 30) x),
          'decisions',(SELECT coalesce(sum(d.n),0)::bigint FROM
            (SELECT DISTINCT event_id AS run_id FROM polymarket.trading_process_events
             WHERE process_id='{pid}' AND event_type='btc_run_manifest'
               AND timestamp_utc >= '{since}'::timestamptz) runs
            CROSS JOIN LATERAL (SELECT count(*) AS n FROM polymarket.btc_strategy_decisions
              WHERE run_id=runs.run_id AND process_id='{pid}'
                AND decision_at >= '{since}'::timestamptz) d),
          'fill_debit',coalesce((SELECT sum(price*size+fee) FROM polymarket.fills WHERE timestamp_utc >= '{since}'::timestamptz AND process_id='{pid}'),0)::text,
          'settlement_credit',(SELECT coalesce(sum(payout),0)::text FROM polymarket.btc_paper_settlement_ledger WHERE process_id='{pid}' AND credit_status='credited'))""")

    @staticmethod
    def economic_snapshot(snapshot):
        return {key: snapshot[key] for key in
                ('orders', 'fills', 'settlements', 'fill_debit', 'settlement_credit')}

    def reuse_processes(self):
        existing = {}
        # Check both identities before updating either definition. Missing/active
        # processes are an operator issue, never permission to create another pair.
        for mode in self.state['modes']:
            pid = REUSABLE_PROCESS_IDS[mode]
            code, response = self.request('GET', '/admin/trading-processes/' + pid)
            if code != 200:
                raise RuntimeError('Reusable ' + mode + ' process is unavailable; no replacement created')
            process = response['process']
            execution = process['execution']
            control = process['btc_realtime_paper']
            if (process['process_id'] != pid or process['enabled'] or process['status'] != 'stopped'
                    or execution['mode'] != mode or execution.get('rail') != {'name': 'polymarket', 'version': 1}):
                raise RuntimeError('Reusable ' + mode + ' process must be stopped with its expected rail contract')
            existing[mode] = process
        for mode, process in existing.items():
            config = copy.deepcopy(process_configuration(process))
            for policy in config['execution']['order_policies']:
                if policy['name'] == 'order_execution_policy':
                    policy['params']['member_order_types']['conservative'] = self.state['order_type']
            control = config['btc_realtime_paper']
            control.get('runtime', {}).pop('trigger', None)
            run_key = 'order-api-' + mode + '-' + self.state['run']
            control['next_experiment_key'] = run_key
            control['preregistration_sha256'] = hashlib.sha256(run_key.encode()).hexdigest()
            code, response = self.request('PATCH', '/admin/trading-processes/' + process['process_id'], config)
            self.check(mode + ' reused new-contract definition', code == 200, response if code != 200 else None)
            self.check(mode + ' stable process identity', response['process']['process_id'] == process['process_id'])
            self.state['processes'][mode] = response['process']
            self.save()

    def compare_policies(self):
        """Read the retained pair and report exact shared-policy differences."""
        definitions = {}
        for mode, pid in REUSABLE_PROCESS_IDS.items():
            code, response = self.request('GET', '/admin/trading-processes/' + pid)
            if code != 200 or response['process']['process_id'] != pid:
                raise RuntimeError('Retained ' + mode + ' definition unavailable')
            execution = response['process']['execution']
            if execution['mode'] != mode or execution['rail'] != {'name': 'polymarket', 'version': 1}:
                raise RuntimeError('Retained process contract changed')
            policies = execution['order_policies']
            if len({p['name'] for p in policies}) != len(policies):
                raise RuntimeError('Duplicate policy identity')
            definitions[mode] = {p['name']: p for p in policies}
        shared = (set(definitions['paper']) | set(definitions['live'])) - {'paper_execution_policy'}
        differences = {name: {mode: definitions[mode].get(name) for mode in definitions}
                       for name in sorted(shared)
                       if definitions['paper'].get(name) != definitions['live'].get(name)}
        result = {'shared_policies_equal': not differences, 'differences': differences,
                  'execution_parity': 'unverified',
                  'paper_adapter_policy': definitions['paper'].get('paper_execution_policy')}
        self.state['policy_comparison'] = result
        self.save()
        print(json.dumps(result, indent=2))
        return result

    def compare_preview(self):
        """Capture one candidate through both existing preview endpoints, without submission."""
        self.compare_policies()
        candidate = self.candidate()
        if candidate is None:
            raise RuntimeError('No current market candidate')
        results = {}
        for mode, pid in REUSABLE_PROCESS_IDS.items():
            code, receipt = self.request('POST', f'/orders/{pid}/preview', candidate)
            results[mode] = {'http_status': code, 'receipt': receipt}
        result = {'candidate': candidate, 'results': results, 'execution_parity': 'unverified'}
        self.state['paired_preview'] = result
        self.save()
        print(json.dumps(result, indent=2))
        return result

    def prepare(self):
        if self.state['processes']:
            raise RuntimeError('Use a new evidence directory for a new run')
        code, baseline = self.request('GET', '/admin/trading-processes?limit=100')
        self.check('baseline process API', code == 200)
        self.state['baseline'] = baseline
        self.state['image'] = json.loads(subprocess.check_output(['kubectl', '--context', 'rancher-desktop',
            '-n', 'capitonic', 'get', 'pods', '-l', 'app.kubernetes.io/name=polymarket-bot', '-o', 'json']))
        self.reuse_processes()
        if 'live' in self.state['processes']:
            p, l = [self.state['processes'][m]['execution']['order_policies'] for m in ('paper','live')]
            self.check('matching shared policy definitions', [x for x in p if x['name'] != 'paper_execution_policy'] == l)
        paper = self.state['processes']['paper']
        variants = {}
        for name in ('duplicate', 'unknown', 'mixed', 'unsupported'):
            config = copy.deepcopy(process_configuration(paper))
            policies = config['execution']['order_policies']
            if name == 'duplicate': policies.append(copy.deepcopy(policies[0]))
            elif name == 'unknown': policies[0]['name'] = 'unsupported_policy'
            elif name == 'mixed': config['execution']['max_open_positions'] = 2
            else: config['execution']['rail']['version'] = 999
            variants[name] = config
        for name, config in variants.items():
            code, response = self.request('PATCH', '/admin/trading-processes/' + paper['process_id'], config)
            self.check('reject ' + name + ' configuration', code == 400, response if code != 400 else None)
        code, response = self.request('GET', '/admin/trading-processes/' + paper['process_id'])
        self.check('invalid updates preserve definition', code == 200 and process_configuration(response['process']) == process_configuration(paper))
        self.start()

    def start(self):
        for mode in self.state['modes']:
            pid = self.state['processes'][mode]['process_id']
            self.state[mode + '_before'] = self.snapshot(pid)
            code, response = self.request('GET', '/admin/trading-processes/' + pid + '/start-preview')
            self.check(mode + ' start preview', code == 200, response if code != 200 else None)
            self.state[mode + '_start_preview'] = response
            candidate = {'version': 1, 'candidate': {'member_id':'conservative','market_id':'absent',
                'outcome':'up','synthetic_prediction':{'probability_up':'0.95'}}}
            code, response = self.request('POST', f'/orders/{pid}/preview', candidate)
            self.check(mode + ' inactive process cannot evaluate', code == 409, response if code != 409 else None)
            code, response = self.request('POST', '/admin/trading-processes/' + pid + '/start')
            self.check(mode + ' normal startup', code == 200, response if code != 200 else None)
        self.save()

    def candidate(self):
        markets = self.sql("SELECT coalesce(json_agg(m),'[]') FROM (SELECT market_id,window_start,window_end FROM polymarket.btc_interval_markets WHERE window_start <= now() AND window_start > now()-interval '5 minutes' AND window_end > now() ORDER BY window_start DESC LIMIT 1) m")
        if not markets:
            return None
        return {'version': 1, 'candidate': {'member_id':'conservative','market_id':markets[0]['market_id'],
            'outcome':'up','synthetic_prediction':{'probability_up':'0.95'}}}

    def evaluate(self):
        candidate = self.candidate()
        assert candidate
        for mode in self.state['modes']:
            pid = self.state['processes'][mode]['process_id']
            before = self.snapshot(pid)
            for route in ('preview','dry-run'):
                code, _ = self.request('POST',f'/orders/{pid}/{route}',candidate,auth=False)
                self.check(mode + ' ' + route + ' authentication',code == 401)
            for field in ('mode','account_ref','quantity','price','rail'):
                invalid = copy.deepcopy(candidate)
                invalid['candidate'][field] = 'forbidden'
                code, _ = self.request('POST',f'/orders/{pid}/preview',invalid)
                self.check(mode + ' rejects override ' + field,code in (400,422))
            for name, value, expected in [('member_id','absent',400),('market_id','absent',503)]:
                invalid = copy.deepcopy(candidate)
                invalid['candidate'][name] = value
                code, _ = self.request('POST',f'/orders/{pid}/preview',invalid)
                self.check(mode + ' unavailable ' + name, code == expected)
            code, _ = self.request('GET',f'/orders/{pid}/order/absent')
            self.check(mode + ' unknown order',code == 404)
            self.check(mode + ' invalid evaluation has no economic writes',before == self.snapshot(pid))
        self.save()

    def poll_preparation(self, seconds):
        deadline = time.monotonic() + seconds
        pending = set(self.state['modes'])
        unavailable = {'paper': 0, 'live': 0}
        while pending and time.monotonic() < deadline:
            candidate = self.candidate()
            if candidate:
                for mode in sorted(pending):
                    pid = self.state['processes'][mode]['process_id']
                    before = self.snapshot(pid)
                    for outcome, probability in [('up','0.95'),('down','0.05')]:
                        candidate['candidate']['outcome'] = outcome
                        candidate['candidate']['synthetic_prediction']['probability_up'] = probability
                        code, preview = self.request('POST',f'/orders/{pid}/preview',candidate)
                        if code == 503:
                            unavailable[mode] += 1
                            if unavailable[mode] >= 6:
                                self.state['blocked'] = {'mode': mode, 'response': preview, 'stage': 'preparation'}
                                self.save()
                                raise RuntimeError('Repeated missing evidence; inspect dependency before retrying')
                        else:
                            unavailable[mode] = 0
                        if code != 200 or preview.get('status') != 'evaluated': continue
                        code, dry = self.request('POST',f'/orders/{pid}/dry-run',candidate)
                        if code != 200 or not dry.get('preparation'): continue
                        self.check(mode + ' preview/dry-run no economic writes',
                                   self.economic_snapshot(before) == self.economic_snapshot(self.snapshot(pid)))
                        self.check(mode + ' no order identity from preparation',preview['order_id'] is None and dry['order_id'] is None)
                        prep = dry['preparation']
                        self.check(mode + ' preparation semantics',prep.get('serialization') == 'performed' and prep.get('signing') == ('performed' if mode == 'live' else 'inapplicable'),prep)
                        forbidden = {'signed_order','signature','private_key','owner','credentials'}
                        self.check(mode + ' redacted preparation',not forbidden.intersection(prep))
                        self.state[mode + '_candidate'] = copy.deepcopy(candidate)
                        self.state[mode + '_dry_run'] = dry
                        pending.remove(mode)
                        self.save()
                        break
            if pending: time.sleep(5)
        self.check('selected modes prepared with real evidence',not pending,sorted(pending))
        self.state.pop('blocked', None)
        self.save()

    def paper(self, seconds):
        pid = self.state['processes']['paper']['process_id']
        if self.state.get('executed') or self.state.get('pending_execute'):
            raise RuntimeError('Recover the recorded request before starting another execution')
        if not all(self.state.get(m + '_dry_run') for m in self.state['modes']):
            raise RuntimeError('Selected preparations must pass before execution qualification')
        attempts = 0
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            candidate = self.candidate()
            if candidate:
                for outcome, probability in [('up','0.95'),('down','0.05')]:
                    candidate['candidate'].update(outcome=outcome,synthetic_prediction={'probability_up':probability})
                    code, preview = self.request('POST',f'/orders/{pid}/preview',candidate)
                    if code == 200 and any(p.get('reason') == 'daily_loss_limit'
                                           for p in preview.get('policy_results', [])):
                        raise RuntimeError('Retained paper process daily loss limit reached; qualification stopped')
                    if code != 200 or preview.get('status') != 'evaluated': continue
                    attempts += 1
                    if attempts > 3:
                        raise RuntimeError('Execution attempt budget exhausted')
                    body = dict(candidate,idempotency_key=str(uuid.uuid4()))
                    # Persist identity before submission so an interrupted driver can recover it.
                    self.state['pending_execute'] = body
                    self.save()
                    code, receipt = self.request('POST',f'/orders/{pid}/execute',body)
                    self.check('paper execute returns durable receipt',code == 200,receipt)
                    self.state['executed'] = receipt
                    self.save()
                    code, retrieved = self.request('GET',f'/orders/{pid}/order/'+urllib.parse.quote(receipt['order_id'],safe=''))
                    self.check('paper retrieval identity',code == 200 and retrieved['operation_id'] == body['idempotency_key'])
                    snapshot = self.snapshot(pid)
                    self.state['paper_after'] = snapshot
                    order = next(x for x in snapshot['orders'] if x['order_id'] == receipt['order_id'])
                    fills = [x for x in snapshot['fills'] if x['order_id'] == receipt['order_id']]
                    self.check('receipt agrees with persisted lifecycle',retrieved['status'] == order['state'])
                    self.check('receipt agrees with canonical fills',{x['fill_id'] for x in retrieved['evidence']['fills']} == {x['fill_id'] for x in fills})
                    if 'live' in self.state['processes']:
                        self.check('live preparation never submits',
                                   self.economic_snapshot(self.snapshot(self.state['processes']['live']['process_id']))
                                   == self.economic_snapshot(self.state['live_before']))
                    code, _ = self.request('GET',f"/orders/{REUSABLE_PROCESS_IDS['live']}/order/"+urllib.parse.quote(receipt['order_id'],safe=''))
                    self.check('foreign order ownership',code == 404)
                    if not fills:
                        self.check('unfilled execution consumes no paper capital',Decimal(snapshot['fill_debit']) == Decimal(self.state['paper_before']['fill_debit']))
                        self.check('unfilled attempt is definitively rejected',receipt['status'] == 'rejected')
                        self.state.pop('executed')
                        self.state.pop('pending_execute')
                        self.save()
                        time.sleep(5)
                        break
                    request = order['raw_payload']['request']
                    self.check('paper fills and order retain canonical ownership',request['process_id'] == pid and all(x['source'] == 'paper' and x['raw_payload']['process_id'] == pid for x in fills))
                    debit = sum(Decimal(x['price'])*Decimal(x['size'])+Decimal(x['fee']) for x in fills)
                    self.check('paper debit equals notional plus actual fees',Decimal(snapshot['fill_debit']) - Decimal(self.state['paper_before']['fill_debit']) == debit)
                    seed = next(x['params']['starting_collateral_usd'] for x in self.state['processes']['paper']['execution']['order_policies'] if x['name']=='paper_execution_policy')
                    self.state['paper_available_collateral'] = str(Decimal(seed)-Decimal(snapshot['fill_debit'])+Decimal(snapshot['settlement_credit']))
                    self.check('paper authorized budget respected',sum(Decimal(x['price'])*Decimal(x['size']) for x in fills) <= Decimal(request['price'])*Decimal(request['size'])+Decimal('0.000001'))
                    self.check('paper price respects authorized limit',all(Decimal(x['price']) <= Decimal(request['price']) for x in fills))
                    self.check('paper fill is explicitly synthetic API origin',request['metadata']['polymarket_execution']['origin'] == 'api')
                    code, replay = self.request('POST',f'/orders/{pid}/execute',body)
                    self.check('paper replay returns original order',code == 200 and replay['order_id'] == receipt['order_id'])
                    self.check('replay has no duplicate economic effects',snapshot == self.snapshot(pid))
                    return
            time.sleep(5)
        self.check('paper fill observed within bounded run',False)

    def settlement(self, seconds):
        pid = self.state['processes']['paper']['process_id']
        order_id = self.state['executed']['order_id']
        route = f'/orders/{pid}/order/' + urllib.parse.quote(order_id, safe='')
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            code, receipt = self.request('GET', route)
            self.check('settlement receipt ownership', code == 200 and receipt['process_id'] == pid
                       and receipt['order_id'] == order_id)
            settlement = receipt['evidence']['accounting']['settlement']
            if settlement is None or settlement['credit_status'] != 'credited':
                time.sleep(5)
                continue
            snapshot = self.snapshot(pid)
            order = next(x['raw_payload']['request'] for x in snapshot['orders'] if x['order_id'] == order_id)
            fills = [x for x in snapshot['fills'] if x['order_id'] == order_id]
            quantity = sum(Decimal(x['size']) for x in fills)
            debit = sum(Decimal(x['price']) * Decimal(x['size']) + Decimal(x['fee']) for x in fills)
            market = order['market_id'].replace("'", "''")
            official = self.sql("SELECT json_build_object('winner',official_winning_token_id,'source',official_resolution_source) "
                                "FROM polymarket.btc_interval_markets WHERE market_id='" + market + "'")
            self.check('official resolution present', official is not None and official['winner'] is not None
                       and official['source'] in ('clob_websocket', 'clob_rest_reconciliation'))
            expected_payout = quantity if order['token_id'] == official['winner'] else Decimal(0)
            self.check('settlement quantity matches fills', quantity > 0 and Decimal(str(settlement['filled_size'])) == quantity)
            self.check('settlement debit matches notional and fees',
                       Decimal(str(settlement['entry_notional'])) + Decimal(str(settlement['entry_fees'])) == ledger_amount(debit))
            self.check('settlement payout matches official winner', Decimal(str(settlement['payout'])) == expected_payout)
            self.check('settlement net pnl matches credit minus debit', Decimal(str(settlement['net_pnl'])) == ledger_amount(expected_payout - debit))
            self.check('one canonical credited settlement', len([x for x in snapshot['settlements']
                       if x['order_id'] == order_id and x['credit_status'] == 'credited']) == 1)
            self.state['verified_settlement'] = settlement
            self.state['settlement_snapshot'] = snapshot
            self.save()
            return
        self.check('official settlement credited within bounded run', False)

    def next_order(self):
        """Archive a verified settled order without restarting its metrics owner."""
        settlement = self.state.get('verified_settlement')
        if not settlement or settlement.get('credit_status') != 'credited':
            raise RuntimeError('Verify settlement before advancing to another paper order')
        pid = self.state['processes']['paper']['process_id']
        snapshot = self.snapshot(pid)
        self.check('settled economics remain unchanged before next order',
                   snapshot == self.state['settlement_snapshot'])
        code, current = self.request('GET', '/admin/trading-processes/' + pid)
        expected = process_configuration(self.state['processes']['paper'])
        self.check('same paper configuration and active metrics owner', code == 200
                   and current['process']['enabled'] and current['process']['status'] == 'running'
                   and process_configuration(current['process']) == expected)
        keys = ('executed', 'pending_execute', 'paper_after', 'verified_settlement', 'settlement_snapshot')
        self.state.setdefault('completed_orders', []).append({key: self.state[key] for key in keys})
        for key in keys:
            del self.state[key]
        self.state['paper_before'] = snapshot
        self.save()

    def latency(self):
        # Capture while process metrics owners still exist; stopping removes their series.
        code, response = self.request('GET', '/prometheus/metrics')
        if code != 200 or 'text' not in response:
            raise RuntimeError('Prometheus exposition unavailable')
        text = response['text']
        current = json.loads(subprocess.check_output(['kubectl', '--context', 'rancher-desktop', '-n', 'capitonic',
            'get', 'pods', '-l', 'app.kubernetes.io/name=polymarket-bot', '-o', 'json']))
        def identities(pods):
            return sorted((p['metadata']['uid'], c['name'], c.get('restartCount', 0), c.get('imageID'))
                          for p in pods['items'] for c in p.get('status', {}).get('containerStatuses', []))
        if not identities(current) or identities(current) != identities(self.state['image']):
            raise RuntimeError('Runtime changed during qualification; cumulative latency samples are not comparable')
        ids = {p['process_id'] for p in self.state['processes'].values()}
        selected = '\n'.join(line for line in text.splitlines()
            if any('process_id="' + pid + '"' in line for pid in ids)) + '\n'
        (self.directory / 'latency.prom').write_text(selected)
        result = latency_report(self.state, selected)
        (self.directory / 'latency.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))

    def stop(self):
        for mode, p in self.state['processes'].items():
            code, current = self.request('GET','/admin/trading-processes/'+p['process_id'])
            if code == 200 and current['process']['enabled']:
                code, result = self.request('POST','/admin/trading-processes/'+p['process_id']+'/stop')
                self.check(mode + ' operator stop',code == 200,result if code != 200 else None)

    def audit(self):
        for mode, p in self.state['processes'].items():
            pid = p['process_id']
            snapshot = self.snapshot(pid)
            self.state[mode + '_final'] = snapshot
            if not self.state.get('executed'):
                self.check(mode + ' blocked evaluation leaves no economic records',
                           snapshot == self.state[mode + '_before'])
            code, current = self.request('GET', '/admin/trading-processes/' + pid)
            self.check(mode + ' disabled after qualification', code == 200 and
                       not current['process']['enabled'] and current['process']['status'] == 'stopped')
            code, _ = self.request('GET', f'/orders/{pid}/order/absent', auth=False)
            self.check(mode + ' order details require authentication', code == 401)
            code, _ = self.request('POST', f'/orders/{pid}/preview', [])
            self.check(mode + ' malformed envelope rejected', code in (400, 422))
        code, current = self.request('GET', '/admin/trading-processes?limit=100')
        self.check('final process inventory available', code == 200)
        by_id = {p['process_id']: p for p in current['processes']}
        for p in self.state['baseline']['processes']:
            if p['enabled']:
                actual = by_id.get(p['process_id'])
                self.check('preserved existing process ' + p['process_id'], actual is not None and
                           process_configuration(actual) == process_configuration(p) and actual['enabled'] and actual['status'] == 'running')
        self.save()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action',choices=['compare-preview','compare-policies','prepare','start','evaluate','prepare-orders','paper','settlement','next-order','latency','stop','audit'])
    parser.add_argument('--evidence-dir',required=True)
    parser.add_argument('--mode',choices=['both','paper'],default='both')
    parser.add_argument('--order-type',choices=['fak','fok'],default='fak')
    parser.add_argument('--timeout',type=int,default=360)
    args = parser.parse_args()
    q = Qualification(args.evidence_dir, args.order_type, args.mode)
    {'compare-preview':q.compare_preview,'compare-policies':q.compare_policies,'prepare':q.prepare,'start':q.start,'evaluate':q.evaluate,'prepare-orders':lambda:q.poll_preparation(args.timeout),
     'paper':lambda:q.paper(args.timeout),'settlement':lambda:q.settlement(args.timeout),'next-order':q.next_order,'latency':q.latency,'stop':q.stop,'audit':q.audit}[args.action]()
