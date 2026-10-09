"""Opt-in HTTP qualification of the deployed order application; never submits live orders.

Uses normal process/order APIs and bounded read-only SQL against local development.
No test implementation, feature flag, or persistence path is installed in the rail.
"""
import argparse
import copy
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[4]


class Qualification:
    def __init__(self, directory, order_type="fak"):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.state_path = self.directory / 'run.json'
        self.state = json.loads(self.state_path.read_text()) if self.state_path.exists() else {
            'run': str(uuid.uuid4()), 'processes': {}, 'checks': [], 'requests': []}
        self.state.setdefault('order_type', order_type)
        self.token = os.environ['POLYMARKET_HTTP_ADMIN_TOKEN']
        self.base = 'http://localhost/api/bot'

    def save(self):
        self.state_path.write_text(json.dumps(self.state, indent=2) + '\n')

    def check(self, name, condition, detail=None):
        self.state['checks'].append({'name': name, 'passed': bool(condition), 'detail': detail})
        self.save()
        print(('PASS ' if condition else 'FAIL ') + name, flush=True)
        if not condition:
            raise AssertionError(f'{name}: {detail}')

    def request(self, method, path, body=None, auth=True):
        # Enforce the funded-test boundary independently of request construction.
        if path.endswith('/execute'):
            pid = path.split('/')[2]
            if pid != self.state['processes']['paper']['process_id']:
                raise RuntimeError('Only the owned paper process may execute')
            code, actual = self.request('GET', '/admin/trading-processes/' + pid)
            if code != 200 or actual['process']['config']['execution']['mode'] != 'paper':
                raise RuntimeError('Current process must be paper before execution')
        headers = {'Content-Type': 'application/json'}
        if auth:
            headers['Authorization'] = 'Bearer ' + self.token
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=45) as r:
                code, raw = r.status, r.read().decode()
        except urllib.error.HTTPError as e:
            code, raw = e.code, e.read().decode()
        try:
            result = json.loads(raw)
        except ValueError:
            result = {'text': raw}
        self.state['requests'].append({'at': datetime.now(timezone.utc).isoformat(),
            'method': method, 'path': path, 'request': body, 'code': code, 'response': result})
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
        return self.sql(f"""SELECT json_build_object(
          'orders',(SELECT coalesce(json_agg(x ORDER BY x.order_id),'[]') FROM
            (SELECT order_id,client_order_id,state,raw_payload FROM polymarket.orders WHERE process_id='{pid}' ORDER BY order_id LIMIT 30) x),
          'fills',(SELECT coalesce(json_agg(x ORDER BY x.fill_id),'[]') FROM
            (SELECT fill_id,order_id,price::text,size::text,fee::text,source,raw_payload FROM polymarket.fills WHERE process_id='{pid}' ORDER BY fill_id LIMIT 30) x),
          'settlements',(SELECT coalesce(json_agg(x ORDER BY x.order_id),'[]') FROM
            (SELECT order_id,payout::text,net_pnl::text,credit_status FROM polymarket.btc_paper_settlement_ledger WHERE process_id='{pid}' ORDER BY order_id LIMIT 30) x),
          'decisions',(SELECT count(*) FROM polymarket.btc_strategy_decisions WHERE process_id='{pid}'),
          'fill_debit',coalesce((SELECT sum(price*size+fee) FROM polymarket.fills WHERE process_id='{pid}'),0)::text,
          'settlement_credit',(SELECT coalesce(sum(payout),0)::text FROM polymarket.btc_paper_settlement_ledger WHERE process_id='{pid}' AND credit_status='credited'))""")

    def prepare(self):
        if self.state['processes']:
            raise RuntimeError('Use a new evidence directory for a new run')
        code, baseline = self.request('GET', '/admin/trading-processes?limit=100')
        self.check('baseline process API', code == 200)
        self.state['baseline'] = baseline
        self.state['image'] = json.loads(subprocess.check_output(['kubectl', '--context', 'rancher-desktop',
            '-n', 'capitonic', 'get', 'pods', '-l', 'app.kubernetes.io/name=polymarket-bot', '-o', 'json']))
        for mode in ('paper', 'live'):
            config = json.loads((ROOT / f'docs/polymarket-execution-rail/examples/{mode}.json').read_text())
            for policy in config['execution']['order_policies']:
                if policy['name'] == 'order_execution_policy':
                    policy['params']['member_order_types']['conservative'] = self.state['order_type']
            control = config['raw']['btc_realtime_paper']
            key = 'order-api-' + mode + '-' + self.state['run']
            control['next_experiment_key'] = key
            control['preregistration_sha256'] = hashlib.sha256(key.encode()).hexdigest()
            body = {'name': 'Order API qualification ' + mode, 'process_type': 'btc_5m',
                    'process_scope': 'realtime_paper', 'enabled': False, 'status': 'created',
                    'config': config, 'metadata': {'qualification_run': self.state['run']}}
            code, response = self.request('PUT', '/admin/trading-processes/by-key/' + key, body)
            self.check(mode + ' new-contract definition', code == 200, response if code != 200 else None)
            self.state['processes'][mode] = response['process']
            self.save()
        p, l = [self.state['processes'][m]['config']['execution']['order_policies'] for m in ('paper','live')]
        self.check('matching shared policy definitions', [x for x in p if x['name'] != 'paper_execution_policy'] == l)
        paper = self.state['processes']['paper']
        variants = {}
        for name in ('duplicate', 'unknown', 'mixed', 'unsupported'):
            config = copy.deepcopy(paper['config'])
            policies = config['execution']['order_policies']
            if name == 'duplicate': policies.append(copy.deepcopy(policies[0]))
            elif name == 'unknown': policies[0]['name'] = 'unsupported_policy'
            elif name == 'mixed': config['execution']['max_open_positions'] = 2
            else: config['execution']['rail']['version'] = 999
            variants[name] = config
        for name, config in variants.items():
            code, response = self.request('PATCH', '/admin/trading-processes/' + paper['process_id'], {'config': config})
            self.check('reject ' + name + ' configuration', code == 400, response if code != 400 else None)
        code, response = self.request('GET', '/admin/trading-processes/' + paper['process_id'])
        self.check('invalid updates preserve definition', code == 200 and response['process']['config'] == paper['config'])
        self.start()

    def start(self):
        for mode in ('paper', 'live'):
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
        for mode in ('paper','live'):
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
        pending = {'paper','live'}
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
                        self.check(mode + ' preview/dry-run no economic writes',before == self.snapshot(pid))
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
        self.check('both modes prepared with real evidence',not pending,sorted(pending))

    def paper(self, seconds):
        pid = self.state['processes']['paper']['process_id']
        if self.state.get('executed') or self.state.get('pending_execute'):
            raise RuntimeError('Recover the recorded request before starting another execution')
        if not all(self.state.get(m + '_dry_run') for m in ('paper', 'live')):
            raise RuntimeError('Both preparations must pass before execution qualification')
        attempts = 0
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            candidate = self.candidate()
            if candidate:
                for outcome, probability in [('up','0.95'),('down','0.05')]:
                    candidate['candidate'].update(outcome=outcome,synthetic_prediction={'probability_up':probability})
                    code, preview = self.request('POST',f'/orders/{pid}/preview',candidate)
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
                    self.check('API activity creates no strategy decisions',snapshot['decisions'] == 0)
                    self.check('live preparation never submits',self.snapshot(self.state['processes']['live']['process_id']) == self.state['live_before'])
                    code, _ = self.request('GET',f"/orders/{self.state['processes']['live']['process_id']}/order/"+urllib.parse.quote(receipt['order_id'],safe=''))
                    self.check('foreign order ownership',code == 404)
                    if not fills:
                        self.check('unfilled execution consumes no paper capital',Decimal(snapshot['fill_debit']) == 0)
                        self.check('unfilled attempt is definitively rejected',receipt['status'] == 'rejected')
                        self.state.pop('executed')
                        self.state.pop('pending_execute')
                        self.save()
                        time.sleep(5)
                        break
                    request = order['raw_payload']['request']
                    self.check('paper fills and order retain canonical ownership',request['process_id'] == pid and all(x['source'] == 'paper' and x['raw_payload']['process_id'] == pid for x in fills))
                    debit = sum(Decimal(x['price'])*Decimal(x['size'])+Decimal(x['fee']) for x in fills)
                    self.check('paper debit equals notional plus actual fees',Decimal(snapshot['fill_debit']) == debit)
                    seed = next(x['params']['starting_collateral_usd'] for x in self.state['processes']['paper']['config']['execution']['order_policies'] if x['name']=='paper_execution_policy')
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
                           actual['config'] == p['config'] and actual['enabled'] and actual['status'] == 'running')
        self.save()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action',choices=['prepare','start','evaluate','prepare-orders','paper','stop','audit'])
    parser.add_argument('--evidence-dir',required=True)
    parser.add_argument('--order-type',choices=['fak','fok'],default='fak')
    parser.add_argument('--timeout',type=int,default=360)
    args = parser.parse_args()
    q = Qualification(args.evidence_dir, args.order_type)
    {'prepare':q.prepare,'start':q.start,'evaluate':q.evaluate,'prepare-orders':lambda:q.poll_preparation(args.timeout),
     'paper':lambda:q.paper(args.timeout),'stop':q.stop,'audit':q.audit}[args.action]()
