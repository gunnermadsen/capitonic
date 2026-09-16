"""Export approved frozen bucket specialists and replay three Q5 routers; no fitting."""
from __future__ import annotations
import argparse, hashlib, json, re
from pathlib import Path
import joblib
import numpy as np
import polars as pl
from .core_extract import file_sha256
from .payoff_runtime_export import _histogram, _base_model
from .runtime_export import canonical_json_bytes, write_immutable_directory
from .time_bucket_specialist_tournament import _opportunities, _select_trades, Policy

RUNS = {
 't2': ('btc-5m-micro-bucket-router-tournament-20260321-20260901','btc-micro-bucket-router-tournament-20260321-20260901','20260914T002735Z','bd50ab8b9bbd0270762115de7831bb78bb4538f7db0a9e3ac53e32c48ee9dfea'),
 't3': ('btc-5m-amalgamated-bucket-tournament-20260321-20260901','btc-amalgamated-bucket-tournament-20260321-20260901','20260914T145421Z','b5a9e33b024959d72980adc22678625f2205355b7e48156af842df3698fae803'),
}
MODELS = [
 ('t2','time_specialist_60_64__with_rtds_candles'),('t2','time_specialist_65_69__with_rtds_candles'),
 ('t2','time_specialist_70_74__without_rtds_candles'),('t2','crossvenue_110_119__without_rtds_candles'),
 ('t3','kraken_120_149__without_rtds_candles'),('t3','price_time_150_169__without_rtds_candles'),
 ('t2','multivenue_185_209__without_rtds_candles'),('t2','kraken_terminal_210_239__without_rtds_candles'),
 ('t2','settlement_terminal_210_239__without_rtds_candles'),
]
GROUPS={'early-middle':[0,1,2,3,4], 'middle-late':[4,5,6,7,8], 'broad-coverage':[1,3,5,6,7]}
SCHEMA='btc-5m-payoff-aware-time-bucket-specialist-features-v1'
KEYS=['market_id','window_start','observed_at','seconds_elapsed']

def contract(names):
    def has(prefix): return any(n.startswith(prefix) for n in names)
    recipes=[
      ('btc_seconds','binance_spot_btcusdt_one_second_ohlcv','binance_closed_seconds_prewindow_open_v1',True,301,5000,True),
      ('execution_book','polymarket_btc_five_minute_orderbooks','causal_vwap_five_shares_v1',True,2,2000,True),
      ('oracle','polygon_chainlink_btcusd_oracle','causal_oracle_rounds_v1',False,600,600000,has(('oracle_','binance_oracle_'))),
      ('refprice','chainlink_btcusd_reference_price','causal_signed_refprice_tournament_v1',True,125,5000,has('chainlink_ref_')),
      ('candles','chainlink_btcusd_one_minute_ohlc','chainlink_ohlc_close_available_120s_v1',True,3660,120000,has('chainlink_candle_')),
      ('open_interest','binance_futures_btcusdt_open_interest','binance_futures_five_minute_open_interest_v1',True,3900,360000,has('binance_oi_')),
      ('kraken','kraken_spot_btcusd_trades','kraken_spot_sparse_trade_seconds_v1',True,121,2000,has('kraken_')),
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
    loaded={};records=[];qualified=[];feature_names=set();panel_cache={}
    manifest=dict(activity='backtests',domain='unified-model-router',workflow='bucket-qualification',run_id=output.name,
       source_branch='feature/unified-model-router',purpose='Immutable Q5 exports and first-qualified array-order replay of approved groups',
       entrypoint='python -m btc_directional_model.router_bucket_export',training_performed=False,
       directories=dict(models='immutable UMR packages',trades='router replay Parquet',metrics='numerical replay summaries',manifests='source/model identities'))
    (output/'README.md').write_text('# Router bucket qualification\n\n'+json.dumps(manifest,indent=2)+'\n')
    for index,(run,name) in enumerate(MODELS):
        folder,old_folder,run_id,sha=RUNS[run]
        work=archive/'runs'/old_folder/run_id
        if run not in loaded:
            artifact_path=source/folder/run_id/'models/tournament.joblib'
            if file_sha256(artifact_path)!=sha:raise ValueError('source artifact identity mismatch')
            loaded[run]=(joblib.load(artifact_path),json.loads((work/'metrics.json').read_text()))
        artifact,report=loaded[run];result=report['candidate_results'][name];model=artifact['models'][name]
        local=[n for i,n in enumerate(model.features) if i not in model.neutralized_columns]
        names=local+[n for n in ['up_ask_vwap_5','down_ask_vwap_5','fee_rate'] if n not in local];feature_names.update(names)
        start,end=map(int,re.search(r'_(\d+)_(\d+)__',name).groups())
        frozen=result['policy'];policy=dict(start_second=start,end_second=end,side=frozen['side'],minimum_confidence=frozen['minimum_confidence'],minimum_edge=frozen['minimum_edge'],maximum_share_cost=frozen['maximum_share_cost'],execution_reserve_per_share=.005)
        key=f'btc-5m-{run}-{name.replace("_","-")}-q5'
        calibration=dict(slope=float(model.calibrator.coef_[0,0]),intercept=float(model.calibrator.intercept_[0])) if model.calibrator is not None else dict(slope=1.,intercept=0.)
        definition=dict(contract=contract(names),outcome=_histogram(model.estimator,local,names,'regression'),calibration=calibration,policy=policy)
        payload=_base_model(key,SCHEMA,names,sha,dict(kind='unified',definition=definition,prediction_policy=dict(type='first_confidence_crossing',minimum_seconds_after_open=start,maximum_seconds_after_open=end,cadence_seconds=1,early_end_second=None,early_cadence_seconds=None,late_start_second=None)))
        payload['provenance'].update(candidate=name,source_training_run=run_id,producing_commit=report['source_commit'],frozen_policy=frozen,qualified_quantity=5,quantity_basis='existing_confirmation_capacity_5',export_is_training=False)
        panel_path=work/'early-causal-panel.parquet' if result['dataset']=='early_causal_panel' else Path(report['source_contract']['paths']['evaluation_panel'])
        scan=pl.scan_parquet(panel_path)
        available=set(scan.collect_schema().names()); missing=set(names)-available
        if missing:raise ValueError(f'{name}: missing panel features {missing}')
        sample=scan.filter(pl.col('seconds_elapsed').is_between(start,end) & pl.col('fee_rate').is_not_null() & pl.col('up_ask_vwap_5').is_not_null() & pl.col('down_ask_vwap_5').is_not_null()).select(list(dict.fromkeys(KEYS+names))).head(128).collect(engine='streaming')
        if sample.is_empty():
            sample=pl.scan_parquet(work/'early-causal-panel.parquet').filter(pl.col('seconds_elapsed').is_between(start,end) & pl.col('fee_rate').is_not_null() & pl.col('up_ask_vwap_5').is_not_null() & pl.col('down_ask_vwap_5').is_not_null()).select(list(dict.fromkeys(KEYS+names))).head(128).collect(engine='streaming')
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
        records.append(dict(index=index,tournament=run,candidate=name,**model_manifest,policy=policy,contract=definition['contract'],reference_cases=len(vectors),original_q5_confirmation=result['confirmation_capacity']['5']))
        predictions=pl.read_parquet(work/'checkpoints'/f'{name}-predictions.parquet')
        # Use the tournament's actual confirmation orderbook capacity evidence.
        execution=pl.scan_parquet(str(work/'confirmation-execution'/'*.parquet')).with_columns(*[((pl.col('observed_at')-pl.col(f'{side}_provider_received_at')).dt.total_microseconds()/1e6).alias(f'pm_{side}_book_age_seconds') for side in ['up','down']]).collect(engine='streaming')
        raw_config={'execution':{'freshness_seconds':2.0,'execution_reserve_per_share':.005,'stress_slippage_per_share':.01}}
        opportunities=_opportunities(execution,predictions,5,raw_config)
        frame=_select_trades(opportunities,Policy(policy['side'],5,policy['minimum_edge'],policy['minimum_confidence'],policy['maximum_share_cost']),raw_config)
        actual=metrics(frame)
        expected=result['confirmation_capacity']['5']
        if actual['trades']!=expected['trades'] or abs(actual['net_pnl']-expected['net_pnl'])>1e-7:
            raise ValueError(f"{name}: archived Q5 replay mismatch: {actual} vs {expected}")
        frame=frame.with_columns(pl.lit(index).alias('member_index'),pl.col('selected_probability').alias('confidence'),(pl.col('side')=='up').alias('up'),pl.col('share_cost').alias('cost'),pl.col('fee_per_share').alias('fee'))
        qualified.append(frame.select(KEYS+['member_index','probability','confidence','up','cost','fee','net_pnl','stress_net_pnl']))
        print(f'exported {key}: {len(vectors)} reference cases',flush=True)
    reports={}
    for group,indices in GROUPS.items():
        all_rows=pl.concat([qualified[i] for i in indices]);priority={i:order for order,i in enumerate(indices)}
        all_rows=all_rows.with_columns(pl.col('member_index').replace_strict(priority).alias('priority'))
        trades=all_rows.sort(['market_id','observed_at','priority']).unique('market_id',keep='first',maintain_order=True).sort('observed_at')
        trades.write_parquet(output/'trades'/f'{group}.parquet')
        reports[group]=dict(**metrics(trades),models=[records[i]['model_key'] for i in indices],quantity=5,period='2026-08-26/2026-09-01',limitations=['Model selection used these historic results; this is not an independent holdout.','Replay uses archived opportunities and execution capacity, not measured live fills.'])
    (output/'manifests/models.json').write_bytes(canonical_json_bytes(records))
    (output/'metrics/routers.json').write_bytes(canonical_json_bytes(reports))
    return feature_names

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--archive',type=Path,required=True);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    export(args.source,args.archive,args.output)
if __name__=='__main__':main()
