use super::{binance_l2_features::BinanceL2FeaturesDrain, retained::RetainedDrainSpec};
use crate::domain::{
    DrainContext, DrainDescriptor, DrainExecutionError, DrainOutcome, DrainRequest,
    DrainWorkerStrategy,
};
use async_trait::async_trait;

const SPEC: RetainedDrainSpec = RetainedDrainSpec {
    key: "binance_futures_btcusdt_l2_one_second_features",
    relation: "market_data.binance_futures_btcusdt_l2_one_second_features",
    schema: "market_data",
    table: "binance_futures_btcusdt_l2_one_second_features",
    retention_days: Some(14),
};
pub struct BinanceFuturesL2OneSecondFeaturesDrain(BinanceL2FeaturesDrain);
impl BinanceFuturesL2OneSecondFeaturesDrain {
    pub fn from_environment() -> Result<Self, DrainExecutionError> {
        BinanceL2FeaturesDrain::from_environment(
            &SPEC,
            "INGESTER_BINANCE_FUTURES_L2_FEATURES_LAKE_ROOT",
            "/var/lib/binance-l2/drains/futures-one-second-features",
            "Binance futures L2 feature",
        )
        .map(Self)
    }
}
#[async_trait]
impl DrainWorkerStrategy for BinanceFuturesL2OneSecondFeaturesDrain {
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
#[path = "tests/binance_futures_l2_one_second_features.rs"]
mod tests;
