use super::{binance_l2_features::BinanceL2FeaturesDrain, retained::RetainedDrainSpec};
use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainOutcome, DrainRequest,
    DrainWorkerStrategy,
};
use async_trait::async_trait;

const SPEC: RetainedDrainSpec = RetainedDrainSpec {
    key: "binance_spot_btcusdt_l2_one_second_features",
    relation: "market_data.binance_spot_btcusdt_l2_one_second_features",
    schema: "market_data",
    table: "binance_spot_btcusdt_l2_one_second_features",
    retention_days: Some(14),
};
pub struct BinanceSpotL2OneSecondFeaturesDrain(BinanceL2FeaturesDrain);
impl BinanceSpotL2OneSecondFeaturesDrain {
    pub fn from_environment() -> Result<Self, DrainExecutionError> {
        BinanceL2FeaturesDrain::from_environment(
            &SPEC,
            "INGESTER_BINANCE_SPOT_L2_FEATURES_LAKE_ROOT",
            "/var/lib/binance-l2/drains/spot-one-second-features",
            "Binance spot L2 feature",
        )
        .map(Self)
    }
}
#[async_trait]
impl DrainWorkerStrategy for BinanceSpotL2OneSecondFeaturesDrain {
    fn descriptor(&self) -> &DrainDescriptor {
        self.0.descriptor()
    }
    fn validate_request(&self, request: &DrainRequest) -> Result<(), DrainExecutionError> {
        self.0.validate_request(request)
    }
    async fn execute_drain(
        &self,
        context: DrainContext,
        request: DrainRequest,
    ) -> Result<DrainOutcome, DrainExecutionError> {
        self.0.execute_drain(context, request).await
    }
}
#[cfg(test)]
#[path = "tests/binance_spot_l2_one_second_features.rs"]
mod tests;
