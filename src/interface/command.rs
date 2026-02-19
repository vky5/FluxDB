use serde_json::Value;
use tokio::sync::oneshot;

use crate::store::kv::Document;

pub enum Command {
    Set {
        key: String,
        value: Value,
        resp: oneshot::Sender<Result<(), String>>,
    },

    Get {
        key: String,
        resp: oneshot::Sender<Option<Document>>,
    },

    Del {
        key: String,
        resp: oneshot::Sender<Result<(), String>>,
    },

    Patch {
        key: String,
        delta: Value,
        resp: oneshot::Sender<Result<(), String>>,
    },
}
