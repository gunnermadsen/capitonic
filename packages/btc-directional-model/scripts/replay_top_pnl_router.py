"""Replay the exact historical Q5 PnL top ten using frozen tournament models."""
from __future__ import annotations

import argparse
from dataclasses import replace
from datetime import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tomllib


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, default=str) + '\n')


def compose(parts, pl):
    # Earliest qualifying entry wins; frozen PnL rank resolves same-second signals.
    return (pl.concat(parts, how='diagonal_relaxed')
            .sort(['market_id', 'seconds_elapsed', 'rank'])
            .unique('market_id', keep='first', maintain_order=True)
            .sort(['window_start', 'seconds_elapsed', 'market_id']))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-worktree', type=Path, required=True)
    parser.add_argument('--archive-package', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source = args.source_worktree.resolve() / 'packages/btc-directional-model'
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    sys.path.insert(0, str(source / 'src'))
    import joblib
    import numpy as np
    import polars as pl
    from btc_directional_model import time_bucket_specialist_tournament as base

    # Fail if a future edit accidentally enters any fitting path.
    def forbidden_fit(*args, **kwargs):
        raise RuntimeError('This replay must never fit a model or policy')
    base._fit_model = forbidden_fit
    base._select_policy = forbidden_fit
    specs = [
        ('btc-5m-time-bucket-specialist-tournament-20260321-20260901', '20260913T215700Z', 'sealed'),
        ('btc-5m-micro-bucket-router-tournament-20260321-20260901', '20260914T002735Z', 'confirmation'),
        ('btc-5m-amalgamated-bucket-tournament-20260321-20260901', '20260914T145421Z', 'confirmation'),
        ('btc-5m-time-bucket-specialist-tournament-20260321-20260914', '20260914T171438Z', 'sealed'),
    ]
    universe, sources = [], []
    for tournament, (folder, run, period) in enumerate(specs, 1):
        directory = source / 'training-results' / folder / run
        metrics_path = directory / 'metrics.json'
        metrics = json.loads(metrics_path.read_text())
        artifact = directory / 'tournament.joblib'
        assert sha(artifact) == metrics['artifact_sha256'], artifact
        sources.append({'tournament': tournament, 'run': run, 'metrics_path': str(metrics_path),
                        'metrics_sha256': sha(metrics_path), 'artifact_path': str(artifact),
                        'artifact_sha256': metrics['artifact_sha256'], 'ranking_period': period})
        for name, candidate in metrics['candidate_results'].items():
            assert candidate.get('form', 'individual') == 'individual'
            score = candidate[period + '_capacity']['5']['net_pnl']
            universe.append({'tournament': tournament, 'name': name, 'q5_pnl': score})
    top = sorted(universe, key=lambda row: (-row['q5_pnl'], row['tournament'], row['name']))[:10]
    assert len(universe) == 145 and len(top) == 10
    assert all(row['tournament'] == 4 for row in top), 'Selection changed; inspect evaluation compatibility'
    directory = Path(sources[3]['metrics_path']).parent
    metrics = json.loads((directory / 'metrics.json').read_text())
    config = source / 'configs/btc-5m-time-bucket-specialist-tournament-20260321-20260914.toml'
    raw = tomllib.loads(config.read_text())
    provenance = json.loads((directory / 'model-provenance.json').read_text())
    assert sha(config) == provenance['configuration_sha256']
    artifact = joblib.load(directory / 'tournament.joblib')
    panel_path = args.archive_package / 'data/btc-full-coverage-vwap-admission-20260321-20260914/vwap-admission-panel.parquet'
    print('Verifying archived panel identity', flush=True)
    panel_hash = sha(panel_path)
    assert panel_hash == provenance['source_hashes']['evaluation_panel']
    contracts = [metrics['candidate_results'][row['name']] for row in top]
    columns = list(base._required_columns(contracts, {'execution': {'quantities': [5, 50], 'capacity_quantities': []}}))
    windows = {name: datetime.fromisoformat(value) for name, value in metrics['windows'].items()}
    scan = pl.scan_parquet(panel_path)
    columns = [name for name in columns if name not in ('label_weight', 'bridge_probability_target')]
    assert set(columns) <= set(scan.collect_schema().names()), sorted(set(columns) - set(scan.collect_schema().names()))
    panel = (scan.filter(pl.col('window_start').is_between(windows['sealed_start'], windows['confirmation_end'], closed='left')
                        & pl.col('seconds_elapsed').is_between(185, 239))
             .select(columns).collect()
             .with_columns(pl.col('window_start').cast(pl.Datetime(time_zone='UTC')),
                           pl.col('observed_at').cast(pl.Datetime(time_zone='UTC'))))
    assert panel.select(base.KEY_COLUMNS).is_duplicated().sum() == 0
    buckets = {b['name']: b for b in metrics['buckets']}
    members = []
    for rank, row in enumerate(top, 1):
        c = metrics['candidate_results'][row['name']]
        members.append({**row, 'rank': rank, 'bucket': buckets[c['bucket']], 'policy': c['policy'],
                        'rtds': c['rtds_mode'], 'features': c['features']})
    manifest = {'selection': 'Exact top ten by Q5 net PnL from the four previously reported ranking periods; no additional filters',
                'candidate_count': len(universe), 'members': members, 'sources': sources,
                'source_code_commit': subprocess.check_output(['git', '-C', str(args.source_worktree), 'rev-parse', 'HEAD'], text=True).strip(),
                'replay_base_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
                'replay_script_sha256': sha(__file__), 'config_sha256': sha(config),
                'panel_path': str(panel_path), 'panel_sha256': panel_hash, 'execution': raw['execution'],
                'windows': metrics['windows'], 'routing': 'Earliest admissible entry per market; PnL rank breaks same-second ties; original side and admission policy; aliases retained',
                'training_performed': False, 'quantities': [5, 50]}
    write_json(output / 'router-manifest.json', manifest)
    parts = {(period, q): [] for period in ('sealed', 'confirmation') for q in (5, 50)}
    member_results, parity = [], []
    for member in members:
        name, rank = member['name'], member['rank']
        print(f'Frozen inference {rank}/10: {name}', flush=True)
        c, model = metrics['candidate_results'][name], artifact['models'][name]
        assert list(model.features) == c['features']
        frame = base._slice(panel, member['bucket'], windows['sealed_start'], windows['confirmation_end'])
        predictions = base._prediction_frame(frame, base._predict(model, frame), name)
        predictions.write_parquet(output / f'{rank:02d}-predictions.parquet')
        for period in ('sealed', 'confirmation'):
            lo, hi = windows[period + '_start'], windows[period + '_end']
            f = frame.filter(pl.col('window_start').is_between(lo, hi, closed='left'))
            p = predictions.filter(pl.col('window_start').is_between(lo, hi, closed='left'))
            for q in (5, 50):
                policy = replace(base.Policy(**c['policy']), quantity=q)
                trades = base._select_trades(base._opportunities(f, p, q, raw), policy, raw)
                actual = base._economic_metrics(trades, f['market_id'].n_unique())
                expected = c['sealed_capacity'][str(q)] if period == 'sealed' else c['confirmation'] if q == 5 else None
                if expected:
                    for key in ['trades', 'wins', 'losses', 'net_pnl', 'stress_net_pnl', 'profit_factor', 'market_coverage', 'average_confidence', 'recovery_wins_per_loss']:
                        assert np.isclose(actual[key], expected[key], rtol=1e-8, atol=1e-7), (name, period, q, key, actual[key], expected[key])
                    parity.append({'name': name, 'period': period, 'quantity': q, 'passed': True})
                trades = trades.with_columns(pl.lit(rank).alias('rank'), pl.lit(name).alias('member'))
                parts[(period, q)].append(trades)
                member_results.append({'name': name, 'rank': rank, 'period': period, 'quantity': q, **actual})
    results, contributions = [], []
    for (period, q), frames in parts.items():
        all_trades = pl.concat(frames, how='diagonal_relaxed')
        router = compose(frames, pl)
        assert router['market_id'].n_unique() == router.height
        assert router.height == all_trades['market_id'].n_unique()
        total_markets = panel.filter(pl.col('window_start').is_between(windows[period+'_start'], windows[period+'_end'], closed='left'))['market_id'].n_unique()
        s = base._economic_metrics(router, total_markets)
        all_trades.write_parquet(output / f'member-trades-{period}-q{q}.parquet')
        router.write_parquet(output / f'router-trades-{period}-q{q}.parquet')
        overlap = all_trades.group_by('market_id').agg(pl.len().alias('signals'), pl.col('side').n_unique().alias('sides'))
        results.append({'period': period, 'quantity': q, 'eligible_markets': total_markets,
                        'member_trade_count_before_deduplication': all_trades.height,
                        'overlapping_markets': overlap.filter(pl.col('signals') > 1).height,
                        'markets_with_conflicting_member_sides': overlap.filter(pl.col('sides') > 1).height, **s})
        for member in members:
            selected = router.filter(pl.col('rank') == member['rank'])
            contributions.append({'period': period, 'quantity': q, 'rank': member['rank'], 'name': member['name'], **base._economic_metrics(selected, total_markets)})
    # Exercise arbitration directly: time dominates rank, rank dominates input order.
    sample = pl.DataFrame({'market_id':['a','a','b','b'], 'seconds_elapsed':[210,185,210,210], 'rank':[1,10,2,1], 'window_start':[1,1,2,2]})
    check = compose([sample], pl)
    assert check['rank'].to_list() == [10,1]
    write_json(output / 'metrics.json', {'router': results, 'members': member_results, 'contributions': contributions,
                                       'verification': {'original_metric_parity_checks': parity, 'arbitration_check': True, 'one_entry_per_market': True}})
    pl.DataFrame(member_results).write_parquet(output / 'member-metrics.parquet')
    pl.DataFrame(contributions).write_parquet(output / 'member-contributions.parquet')
    lines = ['# Top-ten PnL frozen router replay', '', 'Exact top ten from the established four-tournament Q5 PnL ranking. No retraining, extra filters, blending or membership optimization.', '',
             'Selection/sealed: September 1–7, 2026. Subsequent confirmation: September 7–14, 2026. UTC, end exclusive. Selection-period performance is retrospective; confirmation was already observed in prior analysis and is not globally blind.', '',
             'First qualifying entry per market. Same-second conflicts use frozen PnL rank. All original side, bucket, confidence, cost and edge policies retained. Q50 uses actual ladder replay, not linear scaling.', '',
             '## Router results', '', '| Period | Q | PnL | Stress | PF | Trades | W/L | Win rate | Recovery wins/loss | Coverage | Avg confidence | Max DD |', '|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---:|']
    for r in results:
        lines.append(f"| {r['period']} | {r['quantity']} | {r['net_pnl']:.2f} | {r['stress_net_pnl']:.2f} | {r['profit_factor']:.3f} | {r['trades']} | {r['wins']}/{r['losses']} | {100*r['win_rate']:.2f}% | {r['recovery_wins_per_loss']:.3f} | {100*r['market_coverage']:.2f}% | {100*r['average_confidence']:.2f}% | {r['maximum_drawdown']:.2f} |")
    lines += ['', '## Frozen members and attribution at Q5', '', '| Rank | Exact member | Original sealed PnL | Router sealed trades / PnL | Router confirmation trades / PnL |', '|---:|---|---:|---|---|']
    for m in members:
        a,b = [next(r for r in contributions if r['rank']==m['rank'] and r['quantity']==5 and r['period']==period) for period in ('sealed','confirmation')]
        lines.append(f"| {m['rank']} | {m['name']} | {m['q5_pnl']:.2f} | {a['trades']} / {a['net_pnl']:.2f} | {b['trades']} / {b['net_pnl']:.2f} |")
    lines += ['', '## Verification and limitations', '', f'- All four model bundles verified by SHA-256; archived panel and source configuration matched original provenance. {len(parity)} individual period/quantity metric checks reproduced the original results.', '- Ten frozen models re-inferred from causal features; no fitting or policy selection executed. Model duplicates retained and resolved by the same entry arbitration.', '- One entry per market verified. Signals that disagree at different times are included in overlap diagnostics, not necessarily simultaneous conflicts.', '- Recorded VWAP ladder fillability is assumed. Queue position, live latency, capital constraints and competing portfolio positions are not newly simulated. Net PnL includes original fees and reserve; stress adds original per-share slippage.', '- No model tag, runtime export, database mutation, image build, deployment or merge performed.', '', 'Detailed inputs and identities: [router-manifest.json](router-manifest.json). Complete metrics and parity checks: [metrics.json](metrics.json). Trade-level evidence is stored in the accompanying Parquet files.']
    (output / 'report.md').write_text('\n'.join(lines)+'\n')
    print(json.dumps(results, indent=2), flush=True)


if __name__ == '__main__':
    main()
