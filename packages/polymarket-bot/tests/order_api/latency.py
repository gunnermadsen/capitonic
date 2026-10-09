"""Summarize external API timing and existing process-owned Prometheus histograms.

No order execution or runtime behavior belongs in this reporting module.
"""
from collections import defaultdict
import math
import re


def distribution(values):
    values = sorted(values)
    return {'samples': len(values), 'mean_seconds': sum(values) / len(values),
            'max_seconds': values[-1],
            **{name: values[math.ceil(q * len(values)) - 1] if len(values) >= minimum else None
               for name, q, minimum in [('p50_seconds', .5, 2), ('p95_seconds', .95, 20), ('p99_seconds', .99, 100)]}}


def bucket_quantile(buckets, count, q, minimum):
    if count < minimum:
        return None
    previous_bound, previous_count = 0., 0.
    for bound, cumulative in sorted(buckets):
        if cumulative >= q * count:
            if not math.isfinite(bound):
                return None
            if cumulative == previous_count:
                return bound
            return previous_bound + (bound - previous_bound) * (q * count - previous_count) / (cumulative - previous_count)
        previous_bound, previous_count = bound, cumulative
    return None


def report(state, exposition):
    http = defaultdict(list)
    seen = set()
    for request in state['requests']:
        path = request['path'].split('/')
        if len(path) < 4 or path[1] != 'orders' or 'duration_seconds' not in request:
            continue
        kind = path[3]
        if kind == 'execute':
            key = (path[2], (request.get('request') or {}).get('idempotency_key'))
            if key in seen:
                kind = 'execute_replay'
            seen.add(key)
        http[(path[2], kind, request['code'])].append(request['duration_seconds'])
    groups = defaultdict(dict)
    freshness = []
    for line in exposition.splitlines():
        match = re.fullmatch(r'([^ {]+)\{(.*)\} ([^ ]+)', line)
        if not match:
            continue
        name, raw, number = match.groups()
        labels = dict(re.findall(r'(\w+)="([^"]*)"', raw))
        value = float(number)
        if name in ('polymarket_execution_freshness_last_execution_book_age_milliseconds',
                    'polymarket_execution_freshness_last_book_age_milliseconds'):
            freshness.append({'metric': name, **labels, 'milliseconds': value})
        if not name.startswith('polymarket_order_stage_duration_'):
            continue
        key = tuple(labels.get(k) for k in ('process_id', 'execution_mode', 'origin', 'stage', 'outcome'))
        group = groups[key]
        if name.endswith('_bucket'):
            group.setdefault('buckets', []).append((float(labels['le']), value))
        elif name.endswith('_count'):
            group['count'] = value
        elif name.endswith('_sum'):
            group['sum'] = value
        elif name.endswith('_max_seconds'):
            group['max'] = value
    stages = []
    for key, group in sorted(groups.items()):
        n = group.get('count', 0)
        if not n:
            continue
        stages.append({**dict(zip(('process_id', 'execution_mode', 'origin', 'stage', 'outcome'), key)),
            'samples': int(n), 'mean_seconds': group['sum'] / n, 'max_seconds': group['max'],
            **{name: bucket_quantile(group['buckets'], n, q, minimum) for name, q, minimum in
                [('p50_seconds_estimate', .5, 2), ('p95_seconds_estimate', .95, 20), ('p99_seconds_estimate', .99, 100)]}})
    delays = {}
    for mode, process in state['processes'].items():
        for policy in process.get('config', {}).get('execution', {}).get('order_policies', []):
            if policy['name'] == 'paper_execution_policy':
                delays[process['process_id']] = policy['params']['arrival_latency_ms']
    return {'limits': 'Single-owner cumulative scrape; invalid across owner restart. Nested stages are not additive. Bucket percentiles are estimates; null means insufficient samples or overflow. Thresholds: p50=2, p95=20, p99=100; thresholds do not establish statistical confidence.',
        'configured_paper_arrival_ms': delays, 'stages': stages, 'book_freshness': freshness,
        'http': [{'process_id': pid, 'operation': op, 'http_status': status, **distribution(v)}
                 for (pid, op, status), v in http.items()]}
