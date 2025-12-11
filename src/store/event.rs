#[derive(Debug, Clone)]
pub struct Event {
    pub key: String,
    pub old: Value,
    pub new: Value,
    pub version: u64
}


