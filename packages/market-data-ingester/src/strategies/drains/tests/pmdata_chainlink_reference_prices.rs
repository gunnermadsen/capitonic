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
