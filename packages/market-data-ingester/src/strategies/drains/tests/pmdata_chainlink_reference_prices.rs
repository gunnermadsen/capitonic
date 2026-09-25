use super::PmdataChainlinkReferencePricesDrain;
use crate::domain::DrainWorkerStrategy;

#[test]
fn exposes_archive_drain_identity() {
    assert_eq!(super::SPEC.retention_days, Some(0));
    let adapter = PmdataChainlinkReferencePricesDrain::from_environment().unwrap();
    assert_eq!(
        adapter.descriptor().relation.as_ref(),
        "market_data.pmdata_chainlink_btcusd_reference_prices"
    );
}

#[test]
fn direct_and_pmdata_share_the_reference_price_schema() {
    use crate::domain::{contract_for, DatasetKey};

    let schema = super::schema();
    let fields = schema
        .fields()
        .iter()
        .map(|field| field.name().as_str())
        .collect::<Vec<_>>();
    let direct = contract_for(DatasetKey::ChainlinkBtcusdReferencePrices).unwrap();
    let pmdata = contract_for(DatasetKey::PmdataChainlinkBtcusdReferencePrices).unwrap();
    assert_eq!(fields, direct.fields);
    assert_eq!(fields, pmdata.fields);
}
