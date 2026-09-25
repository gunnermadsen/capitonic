use super::ChainlinkReferencePricesDrain;
use crate::domain::DrainWorkerStrategy;

#[test]
fn exposes_direct_reference_drain_identity() {
    assert_eq!(
        super::super::pmdata_chainlink_reference_prices::DIRECT_SPEC.retention_days,
        Some(0)
    );
    assert_eq!(
        super::super::pmdata_chainlink_reference_prices::DIRECT_SPEC.key,
        "chainlink_btcusd_reference_price"
    );
    let adapter = ChainlinkReferencePricesDrain::from_environment().unwrap();
    assert_eq!(
        adapter.descriptor().relation.as_ref(),
        "market_data.chainlink_btcusd_reference_prices"
    );
}
