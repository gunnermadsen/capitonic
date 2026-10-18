"""Opt-in read-only PostgreSQL checks of the application's actual accounting SQL.

ORDER_API_SQL_TESTS=1 uses the existing local qualification SQL reader. Fixtures
are inline CTEs, never tables or application/test branches in execution code.
"""
import json
import os
import re
import unittest

from qualify import Qualification, ROOT

PID = '00000000-0000-0000-0000-000000000001'
OTHER = '00000000-0000-0000-0000-000000000002'


def literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def query(path, name):
    source = (ROOT / path).read_text()
    return re.search(r'const ' + name + r': &str = r#"(.*?)"#;', source, re.S).group(1)


def fixture(name, rows, columns):
    return (name + ' AS MATERIALIZED (SELECT * FROM jsonb_to_recordset(' +
            literal(json.dumps(rows)) + '::jsonb) AS x(' + columns + '))')


def payload(account='primary', resolution=None):
    value = {'request': {'metadata': {'polymarket_execution': {'account_ref': account},
                                     'execution_intent': 'entry'}}}
    if resolution is not None:
        value['execution_resolution'] = resolution
    return value


@unittest.skipUnless(os.environ.get('ORDER_API_SQL_TESTS') == '1', 'opt-in local PostgreSQL qualification')
class AccountingQueries(unittest.TestCase):
    def run_query(self, sql, fixtures, params):
        sql = re.sub(r'\$(\d+)', lambda match: params[int(match[1]) - 1], sql)
        for table in ('orders', 'fills', 'trading_processes', 'btc_paper_settlement_ledger',
                      'trading_process_events', 'btc_strategy_decisions'):
            sql = sql.replace('polymarket.' + table, 'fixture_' + table)
        # Reuse the qualification reader without initializing HTTP credentials/state.
        reader = object.__new__(Qualification)
        return reader.sql("SELECT COALESCE(json_agg(result),'[]'::json) FROM (WITH " +
                          ', '.join(fixtures) + ' SELECT * FROM (' + sql + ') rows) result')

    def test_snapshot_decisions_include_all_owned_runs_once(self):
        reader = object.__new__(Qualification)
        reader.state = {'processes': {'paper': {
            'process_id': PID, 'created_at': '2026-10-09T00:00:00Z'}}}
        reader.sql = lambda sql: sql
        # Exercise the actual snapshot expression with read-only inline fixtures.
        expression = reader.snapshot(PID).split("'decisions',", 1)[1].split(",\n          'fill_debit'", 1)[0]
        events = [{'process_id': owner, 'event_id': run, 'event_type': kind,
                   'timestamp_utc': '2026-10-09T01:00:00Z'}
                  for owner, run, kind in [(PID, PID, 'btc_run_manifest'),
                                           (PID, PID, 'btc_run_manifest'),
                                           (PID, OTHER, 'btc_run_manifest'),
                                           (OTHER, 'unowned', 'btc_run_manifest'),
                                           (PID, 'not-manifest', 'heartbeat')]]
        decisions = [{'process_id': owner, 'run_id': run, 'decision_at': at}
                     for owner, run, at in [(PID, PID, '2026-10-09T02:00:00Z'),
                                            (PID, OTHER, '2026-10-09T03:00:00Z'),
                                            (OTHER, PID, '2026-10-09T02:00:00Z'),
                                            (PID, 'unowned', '2026-10-09T02:00:00Z'),
                                            (PID, 'not-manifest', '2026-10-09T02:00:00Z'),
                                            (PID, PID, '2026-10-08T02:00:00Z')]]
        fixtures = [fixture('fixture_trading_process_events', events,
                            'process_id uuid,event_id text,event_type text,timestamp_utc timestamptz'),
                    fixture('fixture_btc_strategy_decisions', decisions,
                            'process_id uuid,run_id text,decision_at timestamptz')]
        self.assertEqual(self.run_query('SELECT ' + expression + ' AS count', fixtures, []),
                         [{'count': 2}])
        empty = expression.replace(PID, '00000000-0000-0000-0000-000000000099')
        self.assertEqual(self.run_query('SELECT ' + empty + ' AS count', fixtures, []),
                         [{'count': 0}])

    def test_account_ownership_preserves_ambiguous_and_stopped_sleeves(self):
        rows = []
        for owner, state, resolution, account in [
            (OTHER, 'cancelled', None, 'primary'),
            (OTHER, 'expired', None, 'PRIMARY'),
            ('00000000-0000-0000-0000-000000000003', 'cancelled', 'definitive_nonexecution', 'primary'),
            ('00000000-0000-0000-0000-000000000004', 'expired', 'executed', 'primary'),
            ('00000000-0000-0000-0000-000000000005', 'filled', None, 'primary'),
            ('00000000-0000-0000-0000-000000000006', 'rejected', None, 'primary'),
            ('00000000-0000-0000-0000-000000000007', 'submitted', None, 'other'),
        ]:
            rows.append({'process_id': owner, 'state': state, 'raw_payload': payload(account, resolution)})
        fixtures = [fixture('fixture_orders', rows, 'process_id uuid,state text,raw_payload jsonb'),
                    fixture('fixture_trading_processes', [
                        {'process_id': PID, 'enabled': False, 'mode': 'live', 'account_ref': 'primary'},
                        {'process_id': '00000000-0000-0000-0000-000000000008', 'enabled': True,
                         'mode': 'paper', 'account_ref': 'primary'}
                    ], 'process_id uuid,enabled boolean,config jsonb')]
        result = self.run_query(query('packages/polymarket-bot/src/store/order_execution.rs',
                                     'EXECUTION_ACCOUNT_PROCESSES_SQL'), fixtures, [literal('primary')])
        self.assertEqual([row['process_id'] for row in result], [PID, OTHER])

    def test_open_markets_preserve_fills_reservations_and_process_ownership(self):
        rows = [{'process_id': PID, 'order_id': name, 'market_id': name, 'state': state}
                for name, state in [('active', 'created'), ('partial', 'partially_filled'),
                                    ('unsettled', 'filled'), ('settled', 'filled'),
                                    ('cancelled_fill', 'cancelled'), ('rejected_empty', 'rejected')]]
        fills = [{'process_id': PID, 'order_id': name}
                 for name in ['partial', 'unsettled', 'unsettled', 'settled', 'cancelled_fill']]
        fills.append({'process_id': OTHER, 'order_id': 'rejected_empty'})
        fixtures = [fixture('fixture_orders', rows, 'process_id uuid,order_id text,market_id text,state text'),
                    fixture('fixture_fills', fills, 'process_id uuid,order_id text'),
                    fixture('fixture_btc_paper_settlement_ledger', [
                        {'process_id': PID, 'order_id': 'settled', 'credit_status': 'credited'},
                        {'process_id': OTHER, 'order_id': 'unsettled', 'credit_status': 'credited'},
                        {'process_id': PID, 'order_id': 'cancelled_fill', 'credit_status': 'pending'}
                    ], 'process_id uuid,order_id text,credit_status text')]
        result = self.run_query(query('packages/polymarket-bot/src/store/order_execution.rs',
                                     'EXECUTION_OPEN_MARKETS_SQL'), fixtures, [literal(PID) + '::uuid'])
        self.assertEqual({row['market_id'] for row in result}, {'active', 'partial', 'unsettled', 'cancelled_fill'})

    def test_high_water_evidence_preserves_both_modes_causality_and_credit_conflicts(self):
        rows = [{'process_id': PID, 'order_id': name, 'created_at': '2026-10-09T00:00:00Z',
                 'raw_payload': payload()} for name in ['open', 'credited', 'conflict']]
        fills = []
        for index, (owner, source, at) in enumerate([
            (PID, 'paper', '01:00:00'), (PID, 'live', '02:00:00'),
            (OTHER, 'paper', '02:00:00'), (PID, 'paper', '23:00:00')], 1):
            fills.append({'process_id': owner, 'order_id': 'open', 'source': source,
                          'timestamp_utc': '2026-10-09T' + at + 'Z',
                          'fill_id': f'00000000-0000-0000-0000-{index:012}', 'price': .4, 'size': 2, 'fee': .01})
        ledger = []
        for index, (name, pnl) in enumerate([('credited', .5), ('credited', .5), ('conflict', .2), ('conflict', .3)], 1):
            ledger.append({'process_id': PID, 'order_id': name,
                           'settlement_id': f'10000000-0000-0000-0000-{index:012}',
                           'execution_mode': 'paper', 'credited_at': '2026-10-09T03:00:00Z',
                           'credit_status': 'credited', 'filled_size': 2, 'entry_notional': .8,
                           'entry_fees': .01, 'payout': 1, 'net_pnl': pnl})
        fixtures = [fixture('fixture_orders', rows, 'process_id uuid,order_id text,created_at timestamptz,raw_payload jsonb'),
                    fixture('fixture_fills', fills, 'process_id uuid,order_id text,source text,timestamp_utc timestamptz,fill_id uuid,price numeric,size numeric,fee numeric'),
                    fixture('fixture_btc_paper_settlement_ledger', ledger, 'process_id uuid,order_id text,settlement_id uuid,execution_mode text,credited_at timestamptz,credit_status text,filled_size numeric,entry_notional numeric,entry_fees numeric,payout numeric,net_pnl numeric')]
        params = [literal(PID) + '::uuid'] + [literal(v) + '::timestamptz' for v in
                  ['2026-10-09T12:00:00Z', '2026-10-09T00:00:00Z', '2026-10-10T00:00:00Z']]
        result = self.run_query(query('packages/polymarket-bot/src/btc/repository.rs',
                                     'DAILY_HIGH_WATER_MARK_EVIDENCE_SQL'), fixtures, params)
        open_row = next(row for row in result if row['evidence_kind'] == 'unsettled')
        self.assertEqual(open_row['amount_usd'], 1.62)
        self.assertEqual(open_row['fill_ids'], [fills[0]['fill_id'], fills[1]['fill_id']])
        self.assertEqual(len([row for row in result if row['evidence_kind'] == 'credited']), 2)
        self.assertEqual(next(row['amount_usd'] for row in result if row['evidence_kind'] == 'conflicting_credit'), 2)
        empty = self.run_query(query('packages/polymarket-bot/src/btc/repository.rs',
                                    'DAILY_HIGH_WATER_MARK_EVIDENCE_SQL'), fixtures,
                               [literal('00000000-0000-0000-0000-000000000099') + '::uuid'] + params[1:])
        self.assertEqual(empty, [])


if __name__ == '__main__':
    unittest.main()
