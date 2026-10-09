//! Polymarket execution contracts. Configuration admission is available; execution is not yet registered.
//! See `docs/polymarket-execution-rail/README.md` for ownership and compatibility rules.
pub mod configuration;
pub mod policies;

pub const RAIL_CONTRACT_VERSION: &str = "1.0.0";
pub const OMS_CONTRACT_VERSION: &str = "1.0.0";
pub const PROCESS_SCHEMA_VERSION: &str = "btc_realtime_paper_process_v6";
pub const EXECUTION_UNAVAILABLE: &str = "polymarket_execution_not_implemented";
