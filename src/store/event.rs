use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub key: String,
    pub old: Value,
    pub new: Value,
    pub version: u64,
}
