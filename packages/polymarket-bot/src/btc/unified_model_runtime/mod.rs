//! Stable model integration boundaries; see docs/unified-model-runtime/README.md.
//! Shared feeds and process execution remain owned by the existing BTC runtime.
pub mod adapters;
pub mod agent;
pub mod catalog;
pub mod contract;
pub mod risk;
pub mod router;
pub mod telemetry;
#[cfg(test)]
mod tests;
