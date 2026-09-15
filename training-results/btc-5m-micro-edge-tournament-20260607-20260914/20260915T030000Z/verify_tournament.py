"""Verify completed tournament evidence without fitting or changing any model."""
import json
from datetime import datetime,timedelta
from pathlib import Path
import joblib
import numpy as np
import polars as pl
from btc_directional_model.core_extract import file_sha256
from btc_directional_model.micro_edge_tournament import OUTPUT,load_panel,identity_matches,raw_predict
from btc_directional_model.micro_edge_models import recipes,matrix,predict_estimator,calibrate
from btc_directional_model.micro_edge_data import START,END

frame,manifest=load_panel()
active=json.loads((OUTPUT/'run-identity.json').read_text())
registry=recipes();assert len(registry)==35 and len({r.name for r in registry})==35
assert frame['window_start'].min()==START
assert frame['window_start'].max().date()==(END-timedelta(days=1)).date()
assert frame.height==1705440 and frame['market_id'].n_unique()==28424
assert sum(r['rows'] for r in manifest['selected_raw_partitions']['labels'].values())==28424
sample=frame[np.linspace(0,frame.height-1,1000,dtype=int)]
models={r.name:joblib.load(OUTPUT/'models'/f'{r.name}.joblib') for r in registry}

def final_probability(name):
    model=models[name];features=model['feature_names'];base=model.get('base_model')
    if base:
        basep,_=final_probability(base)
        assert features[-1]=='base_probability_oof'
        x=np.column_stack([matrix(sample,features[:-1]),basep])
    else:x=matrix(sample,features)
    kind=model['recipe']['kind']
    if kind=='wait':return basep,predict_estimator(model['estimator'],x)>=0
    cost=sample['up_ask_vwap_5'].to_numpy();fee=sample['fee_rate'].to_numpy()*cost*(1-cost)
    raw=raw_predict(model['estimator'],x,cost,fee)
    p=calibrate(raw,model['calibrator'],sample['settlement_regime'].to_numpy())
    return p,np.isfinite(p)

results=[];fold_count=0
for recipe in registry:
    name=recipe.name;path=OUTPUT/'models'/f'{name}.joblib';meta=json.loads(path.with_suffix('.json').read_text());model=models[name]
    assert file_sha256(path)==meta['model_artifact_sha256']
    assert identity_matches(meta['source_identity'],active)
    assert model['quantity']==5 and model['training_start']==START.isoformat() and model['training_end_exclusive']==END.isoformat()
    assert not set(model['feature_names'])&{'label_up','official_outcome','final_price','bridge_probability_target','net_pnl','stress_net_pnl'}
    assert datetime.fromisoformat(meta['fit_last']).date()==(END-timedelta(days=1)).date()
    if recipe.kind!='wait':assert datetime.fromisoformat(meta['fit_first']).date()==START.date()
    assert recipe.rtds==any('chainlink_candle_' in c for c in model['feature_names'])
    p,g=final_probability(name);models[name]=joblib.load(path);p2,g2=final_probability(name)
    np.testing.assert_allclose(p,p2,rtol=0,atol=0,equal_nan=True);np.testing.assert_array_equal(g,g2)
    assert np.all((p[np.isfinite(p)]>=0)&(p[np.isfinite(p)]<=1))
    predictions=pl.read_parquet(OUTPUT/'predictions'/f'{name}.parquet')
    assert predictions['row_id'].n_unique()==predictions.height
    assert predictions.filter((pl.col('window_start')<pl.datetime(2026,6,21,time_zone='UTC'))|(pl.col('window_start')>=END)).height==0
    for cp in sorted((OUTPUT/'checkpoints'/name).glob('*/completion.json')):
        saved=json.loads(cp.read_text());assert identity_matches(saved['identity'],active)
        assert file_sha256(cp.parent/'model.joblib')==saved['model_sha256']
        assert file_sha256(cp.parent/'predictions.parquet')==saved['prediction_sha256']
        if saved['train_rows']:
            assert datetime.fromisoformat(saved['train_last_end'])+timedelta(minutes=5)<datetime.fromisoformat(saved['evaluation_start'])
        admitted=cp.parent/'admitted-buckets.parquet'
        if admitted.exists():
            a=pl.read_parquet(admitted);assert a.select('market_id','bucket').n_unique()==a.height
        fold_count+=1
    ledger=pl.read_parquet(OUTPUT/'trades'/f'{name}.parquet')
    assert ledger['market_id'].n_unique()==ledger.height
    assert ledger.filter(pl.col('window_start')<pl.datetime(2026,7,5,time_zone='UTC')).height==0
    source=frame.select('market_id','observed_at','fee_rate','up_ask_vwap_5','down_ask_vwap_5','pm_up_book_age_seconds','pm_down_book_age_seconds',pl.col('label_up').alias('source_label_up'))
    joined=ledger.join(source,on=['market_id','observed_at'],how='left',validate='1:1')
    np.testing.assert_array_equal(joined['label_up'].to_numpy(),joined['source_label_up'].to_numpy())
    up=joined['side'].to_numpy()=='up';cost=np.where(up,joined['up_ask_vwap_5'].to_numpy(),joined['down_ask_vwap_5'].to_numpy());age=np.where(up,joined['pm_up_book_age_seconds'].to_numpy(),joined['pm_down_book_age_seconds'].to_numpy())
    assert np.all(np.isfinite(cost)&(cost>0)&(cost<1)&(age>=0)&(age<=2))
    won=up==(joined['label_up'].to_numpy()==1);fee=joined['fee_rate'].to_numpy()*cost*(1-cost)
    np.testing.assert_allclose(joined['share_cost'].to_numpy(),cost,atol=1e-7,rtol=0)
    np.testing.assert_allclose(joined['net_pnl'].to_numpy(),5*(won.astype(float)-cost-fee-.005),atol=2e-6,rtol=0)
    np.testing.assert_allclose(joined['stress_net_pnl'].to_numpy(),joined['net_pnl'].to_numpy()-.05,atol=1e-10,rtol=0)
    results.append({'name':name,'artifact_sha256':meta['model_artifact_sha256'],'producing_commit':meta['producing_commit'],'fit_rows':meta['fit_rows'],'fit_markets':meta['fit_markets'],'prediction_rows':predictions.height,'trades':ledger.height,'rtds':recipe.rtds,'composed_serialization_parity':True})
for router_name in ('eligible_first','opportunity_reservation','historical_baseline_refits'):
    path=OUTPUT/f'router-{router_name}.parquet'
    ledger=pl.read_parquet(path);assert ledger['market_id'].n_unique()==ledger.height
assert pl.read_parquet(OUTPUT/'model-metrics.parquet').height==35
assert pl.read_parquet(OUTPUT/'bucket-metrics.parquet').height==1050
assert pl.read_parquet(OUTPUT/'five-second-metrics.parquet').height==2100
assert (OUTPUT/'report.md').exists() and (OUTPUT/'coverage-report.md').exists()
(OUTPUT/'verification.json').write_text(json.dumps({'status':'passed','models':results,'fold_checkpoints_verified':fold_count,'source_rows':frame.height,'source_markets':frame['market_id'].n_unique(),'quantity':5,'label_embargo_minutes':5,'full_range_retained':True,'runtime_deployment_performed':False},indent=2)+'\n')
print('VERIFIED',len(results),'models',fold_count,'folds; actual Q5 ledgers and composed artifact reload parity',flush=True)
