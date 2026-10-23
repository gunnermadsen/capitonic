//! Polymarket order contracts, shared OMS and callable execution lifecycle.
//! Runtime process routing and the generic order HTTP controller are not yet registered.
//! See `docs/polymarket-execution-rail/README.md` for ownership and compatibility rules.
pub mod configuration;
pub mod policies;

pub const RAIL_CONTRACT_VERSION: &str = "1.0.0";
pub const OMS_CONTRACT_VERSION: &str = "1.0.0";
pub const PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v6";
pub const EXECUTION_UNAVAILABLE: &str = "polymarket_execution_not_implemented";

pub mod lifecycle;
pub mod oms;
pub mod service;
pub mod settings;

#[cfg(test)]
mod tests;

mod admission_report;

pub use admission_report::AdmissionRejection;
