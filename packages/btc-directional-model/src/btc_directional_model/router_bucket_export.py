"""Export approved frozen bucket specialists and compose three Q5 routers; no fitting."""
from __future__ import annotations
import argparse, hashlib, json
from pathlib import Path
import joblib
import numpy as np
import polars as pl
from .core_extract import file_sha256
from .payoff_runtime_export import _histogram, _base_model
from .runtime_export import canonical_json_bytes, write_immutable_directory
from .time_bucket_specialist_tournament import _opportunities, _select_trades, Policy

RUNS = {
 't1': ('btc-5m-time-bucket-specialist-tournament-20260321-20260901','btc-time-bucket-specialist-tournament-20260321-20260901','20260913T215700Z','1987c3d1e5a82bae587574f4f5bd44a5027cbe25b1f6cb992d0da38c410ea6a6'),
 't2': ('btc-5m-micro-bucket-router-tournament-20260321-20260901','btc-micro-bucket-router-tournament-20260321-20260901','20260914T002735Z','bd50ab8b9bbd0270762115de7831bb78bb4538f7db0a9e3ac53e32c48ee9dfea'),
 't3': ('btc-5m-amalgamated-bucket-tournament-20260321-20260901','btc-amalgamated-bucket-tournament-20260321-20260901','20260914T145421Z','b5a9e33b024959d72980adc22678625f2205355b7e48156af842df3698fae803'),
 't4': ('btc-5m-time-bucket-specialist-tournament-20260321-20260914','btc-time-bucket-specialist-tournament-20260321-20260914','20260914T171438Z','6cabcb197f4c6feda26716c0a61863339e0efa32c084b1ee57749ff0c6640e13'),
}
MODELS = [
 ('t2','bridge_aware_60_64__without_rtds_candles',60,64),
 ('t2','bridge_aware_65_69__without_rtds_candles',65,69),
 ('t2','bridge_aware_70_74__without_rtds_candles',70,74),
 ('t4','bridge_aware_specialist__without_rtds_candles',60,74),
 ('t3','bridge_aware_60_74__without_rtds_candles',60,74),
 ('t1','bridge_aware_specialist__without_rtds_candles',60,74),
 ('t2','extended_official_75_89__without_rtds_candles',75,89),
 ('t2','dual_head_75_89__without_rtds_candles',75,89),
 ('t4','extended_specialist_official__without_rtds_candles',75,89),
 ('t4','specialist_dual_head__without_rtds_candles',75,89),
 ('t1','extended_specialist_official__without_rtds_candles',75,89),
 ('t1','specialist_dual_head__without_rtds_candles',75,89),
 ('t3','price_control_terminal_210_239__without_rtds_candles',210,239),
 ('t2','price_control_terminal_210_239__without_rtds_candles',210,239),
 ('t1','price_control_terminal__without_rtds_candles',210,239),
]
GROUPS={'early-middle':[0,1,2,3,4], 'middle-late':[5,6,7,8,9], 'broad-coverage':[10,11,12,13,14]}
SCHEMA='btc-5m-payoff-aware-time-bucket-specialist-features-v1'
KEYS=['market_id','window_start','observed_at','seconds_elapsed']

def contract(names):
    def has(prefix): return any(n.startswith(prefix) for n in names)
    unsupported=('chainlink_ref_','chainlink_candle_','binance_oi_','kraken_')
    if any(n.startswith(unsupported) or n in {'refprice_margin_bps','sensor_source_age_seconds'} for n in names):
        raise ValueError('model requires an unavailable realtime source')
    recipes=[
      ('btc_seconds','binance_spot_btcusdt_one_second_ohlcv','binance_closed_seconds_prewindow_open_v1',True,301,5000,True),
      ('execution_book','polymarket_btc_five_minute_orderbooks','causal_vwap_five_shares_v1',True,2,2000,True),
      ('oracle','polygon_chainlink_btcusd_oracle','causal_oracle_rounds_v1',False,600,600000,has(('oracle_','binance_oracle_'))),
    ]
    return dict(version='capitonic-unified-model-runtime-v1',adapter='time_bucket_specialist',adapter_version=1,
      inputs=[dict(slot=s,product=p,semantics=r,required=req,lookback_seconds=h,maximum_age_ms=a) for s,p,r,req,h,a,use in recipes if use],
      probability_semantics='probability_up',feature_clock='closed_binance_second_as_of',missing_policy='native_missing_branch',qualified_trade_size=5.0)

def metrics(frame):
    pnl=frame['net_pnl'].to_numpy(); wins=int((pnl>0).sum()); losses=int((pnl<0).sum())
    gross_win=float(pnl[pnl>0].sum());gross_loss=float(-pnl[pnl<0].sum())
    curve=np.cumsum(pnl); drawdown=float(np.max(np.maximum.accumulate(np.r_[0.,curve])[1:]-curve)) if len(pnl) else 0.
    return dict(trades=len(pnl),wins=wins,losses=losses,win_rate=wins/len(pnl) if len(pnl) else None,
      net_pnl=float(pnl.sum()),stress_net_pnl=float(frame['stress_net_pnl'].sum()),profit_factor=gross_win/gross_loss if gross_loss else None,
      recovery_wins_per_loss=(gross_loss/losses)/(gross_win/wins) if losses and wins and gross_win else None,maximum_drawdown=drawdown)

def export(source, archive, output):
    output.mkdir(parents=True,exist_ok=True)
    for directory in ('models','metrics','manifests'):
        (output/directory).mkdir(exist_ok=True)
    loaded={};records=[];feature_names=set()
    manifest=dict(activity='backtests',domain='unified-model-router',workflow='bucket-qualification',run_id=output.name,
       source_branch='feature/unified-model-router',purpose='Immutable Q5 exports and first-qualified array-order composition of approved groups',
       entrypoint='python -m btc_directional_model.router_bucket_export',training_performed=False,
       directories=dict(models='immutable UMR packages',metrics='per-member Q5 evidence and router compositions',manifests='source/model identities'))
    (output/'README.md').write_text('# Router bucket qualification\n\n'+json.dumps(manifest,indent=2)+'\n')
    for index,(run,name,start,end) in enumerate(MODELS):
        folder,old_folder,run_id,sha=RUNS[run]
        work=archive/'runs'/old_folder/run_id
        if run not in loaded:
            artifact_path=source/folder/run_id/'models/tournament.joblib'
            if file_sha256(artifact_path)!=sha:raise ValueError('source artifact identity mismatch')
            loaded[run]=(joblib.load(artifact_path),json.loads((work/'metrics.json').read_text()))
        artifact,report=loaded[run];result=report['candidate_results'][name];model=artifact['models'][name]
        local=[n for i,n in enumerate(model.features) if i not in model.neutralized_columns]
        names=local+[n for n in ['up_ask_vwap_5','down_ask_vwap_5','fee_rate'] if n not in local];feature_names.update(names)
        frozen=result['policy'];policy=dict(start_second=start,end_second=end,side=frozen['side'],minimum_confidence=frozen['minimum_confidence'],minimum_edge=frozen['minimum_edge'],maximum_share_cost=frozen['maximum_share_cost'],execution_reserve_per_share=.005)
        key=f'btc-5m-{run}-{name.replace("_","-")}-q5'
        calibration=dict(slope=float(model.calibrator.coef_[0,0]),intercept=float(model.calibrator.intercept_[0])) if model.calibrator is not None else dict(slope=1.,intercept=0.)
        definition=dict(contract=contract(names),outcome=_histogram(model.estimator,local,names,'regression'),calibration=calibration,policy=policy)
        payload=_base_model(key,SCHEMA,names,sha,dict(kind='unified',definition=definition,prediction_policy=dict(type='first_confidence_crossing',minimum_seconds_after_open=start,maximum_seconds_after_open=end,cadence_seconds=1,early_end_second=None,early_cadence_seconds=None,late_start_second=None)))
        payload['provenance'].update(candidate=name,source_training_run=run_id,producing_commit=report['source_commit'],frozen_policy=frozen,qualified_quantity=5,quantity_basis='existing_confirmation_capacity_5',export_is_training=False)
        panel_path=source/folder/run_id/'datasets/early-causal-panel.parquet'
        scan=pl.scan_parquet(panel_path)
        available=set(scan.collect_schema().names()); missing=set(names)-available
        if missing:raise ValueError(f'{name}: missing panel features {missing}')
        sample=scan.filter(pl.col('seconds_elapsed').is_between(start,end) & pl.col('fee_rate').is_not_null() & pl.col('up_ask_vwap_5').is_not_null() & pl.col('down_ask_vwap_5').is_not_null()).select(list(dict.fromkeys(KEYS+names))).head(128).collect(engine='streaming')
        if sample.is_empty():
            sample=pl.scan_parquet(source/'evaluation-panel.parquet').filter(pl.col('seconds_elapsed').is_between(start,end) & pl.col('fee_rate').is_not_null() & pl.col('up_ask_vwap_5').is_not_null() & pl.col('down_ask_vwap_5').is_not_null()).select(list(dict.fromkeys(KEYS+names))).head(128).collect(engine='streaming')
        x=sample.select(local).to_numpy().astype(float);raw=np.clip(model.estimator.predict(x),1e-6,1-1e-6)
        probabilities=model.calibrator.predict_proba(np.log(raw/(1-raw)).reshape(-1,1))[:,1] if model.calibrator is not None else raw
        vectors=[]
        for i,(row,p) in enumerate(zip(sample.to_dicts(),probabilities)):
            p=float(p);confidence=max(p,1-p);side='up' if p>=.5 else 'down';cost=row[f'{side}_ask_vwap_5'];fee=row['fee_rate']
            if fee is None or not np.isfinite(fee):continue
            accepted=cost is not None and np.isfinite(cost) and 0<cost<=policy['maximum_share_cost'] and confidence>=policy['minimum_confidence'] and confidence-cost-fee*cost*(1-cost)-.005>=policy['minimum_edge'] and policy['side'] in ['both',side]
            vectors.append(dict(id=f'{name}-{i}',seconds_elapsed=int(row['seconds_elapsed']),source=dict(accepted=bool(accepted)),feature_values=[float(row[n]) if row[n] is not None and np.isfinite(row[n]) else None for n in names],expected=dict(probability_up=p,confidence=confidence,raw_logit=float(np.log(p/(1-p))),action=side if accepted else 'no_trade')))
        if not vectors:raise ValueError('empty parity vectors')
        data=canonical_json_bytes(payload);gold=canonical_json_bytes(dict(schema_version='capitonic-btc-payoff-aware-golden-vectors-v1',model_key=key,feature_schema_sha256=payload['features']['schema_sha256'],vectors=vectors))
        model_manifest=dict(schema_version='capitonic-btc-directional-runtime-manifest-v1',model_key=key,model_file='model.json',model_sha256=hashlib.sha256(data).hexdigest(),golden_vectors_file='golden-vectors.json',golden_vectors_sha256=hashlib.sha256(gold).hexdigest(),feature_schema_version=SCHEMA,feature_schema_sha256=payload['features']['schema_sha256'],source_freeze_manifest_sha256=file_sha256(work/'metrics.json'),source_training_model_sha256=sha,deployment_scope='paper_only',production_qualified=False,live_capital_allowed=False)
        write_immutable_directory(output/'models'/key,{'model.json':data,'manifest.json':canonical_json_bytes(model_manifest),'golden-vectors.json':gold})
        evidence=(result.get('confirmation_capacity') or result.get('sealed_capacity'))['5']
        evidence_period='confirmation' if result.get('confirmation_capacity') else 'sealed'
        records.append(dict(index=index,tournament=run,candidate=name,**model_manifest,policy=policy,contract=definition['contract'],reference_cases=len(vectors),q5_evidence_period=evidence_period,original_q5_evidence=evidence))
        raw_config={'execution':{'freshness_seconds':2.0,'execution_reserve_per_share':.005,'stress_slippage_per_share':.01}}
        if result.get('confirmation_capacity'):
            predictions=pl.read_parquet(work/'checkpoints'/f'{name}-predictions.parquet')
            # Use the tournament's actual confirmation orderbook capacity evidence.
            execution=pl.scan_parquet(str(work/'confirmation-execution'/'*.parquet')).with_columns(*[((pl.col('observed_at')-pl.col(f'{side}_provider_received_at')).dt.total_microseconds()/1e6).alias(f'pm_{side}_book_age_seconds') for side in ['up','down']]).collect(engine='streaming')
            opportunities=_opportunities(execution,predictions,5,raw_config)
            frame=_select_trades(opportunities,Policy(policy['side'],5,policy['minimum_edge'],policy['minimum_confidence'],policy['maximum_share_cost']),raw_config)
            actual=metrics(frame)
            if actual['trades']!=evidence['trades'] or abs(actual['net_pnl']-evidence['net_pnl'])>1e-7:
                raise ValueError(f"{name}: archived Q5 replay mismatch: {actual} vs {evidence}")
        print(f'exported {key}: {len(vectors)} reference cases',flush=True)
    reports={}
    for group,indices in GROUPS.items():
        reports[group]=dict(models=[records[i]['model_key'] for i in indices],quantity=5,
          member_q5_evidence=[dict(model_key=records[i]['model_key'],period=records[i]['q5_evidence_period'],metrics=records[i]['original_q5_evidence']) for i in indices],
          composition_replay='not_performed_mixed_evidence_periods',limitations=['Model selection used these historic results; this is not an independent holdout.','The four tournaments do not retain a common-period Q5 trade ledger, so aggregate router PnL is intentionally not reported.','Archived opportunities and capacity evidence are not measured live fills.'])
    (output/'manifests/models.json').write_bytes(canonical_json_bytes(records))
    (output/'metrics/routers.json').write_bytes(canonical_json_bytes(reports))
    return feature_names

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--archive',type=Path,required=True);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    export(args.source,args.archive,args.output)
if __name__=='__main__':main()
