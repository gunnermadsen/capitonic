//! Process-scoped compiled TPC publication. Readers retain one immutable revision per operation.
use super::oms::Oms;
use std::sync::{Arc, RwLock};

pub struct ProcessSettings {
    current: RwLock<Arc<SettingsSnapshot>>,
}
pub struct SettingsSnapshot {
    pub(crate) oms: Arc<Oms>,
    pub(crate) configuration: Option<serde_json::Value>,
}
impl ProcessSettings {
    pub(crate) fn new(oms: Arc<Oms>) -> Self {
        Self {
            current: RwLock::new(Arc::new(SettingsSnapshot {
                oms,
                configuration: None,
            })),
        }
    }
    pub(crate) fn snapshot(&self) -> Arc<SettingsSnapshot> {
        self.current.read().expect("process settings lock").clone()
    }
    pub(crate) fn publish(&self, snapshot: SettingsSnapshot) {
        *self.current.write().expect("process settings lock") = Arc::new(snapshot);
    }
}
