use serde_json::Value;
use tokio::sync::oneshot;

use crate::store::{kv::Document, snapshot::Snapshot};

pub enum ReadCommand {
    Get {
        key: String,
        resp: oneshot::Sender<Option<Document>>,
    },
}

pub enum WriteCommand {
    Set {
        key: String,
        value: Value,
        resp: oneshot::Sender<Result<(), String>>,
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
    Snapshot {
        resp: oneshot::Sender<Result<Snapshot, String>>,
    },
    InjectFailure {
        resp: oneshot::Sender<()>,
    },
}
