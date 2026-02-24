use tokio::sync::mpsc;

use crate::{
    interface::{
        command::{ReadCommand, WriteCommand},
    },
    store::kv::Document,
};
use serde_json::Value;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct EngineHandle {
    read_tx: mpsc::Sender<ReadCommand>,
    write_tx: mpsc::Sender<WriteCommand>,
}

impl EngineHandle {
    pub fn new(read_tx: mpsc::Sender<ReadCommand>, write_tx: mpsc::Sender<WriteCommand>) -> Self {
        Self { read_tx, write_tx }
    }

    pub async fn set(&self, key: String, value: Value) -> Result<(), String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.write_tx
            .send(WriteCommand::Set {
                key,
                value,
                resp: resp_tx,
            })
            .await
            .map_err(|_| "writer dropped".to_string())?;
        resp_rx.await.map_err(|_| "writer dropped".to_string())?
    }

    pub async fn patch(&self, key: String, delta: Value) -> Result<(), String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.write_tx
            .send(WriteCommand::Patch {
                key,
                delta,
                resp: resp_tx,
            })
            .await
            .map_err(|_| "writer dropped".to_string())?;

        resp_rx.await.map_err(|_| "writer dropped".to_string())?
    }

    pub async fn delete(&self, key: String) -> Result<(), String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.write_tx
            .send(WriteCommand::Del { key, resp: resp_tx })
            .await
            .map_err(|_| "writer dropped".to_string())?;

        resp_rx.await.map_err(|_| "writer dropped".to_string())?
    }

    pub async fn get(&self, key: String) -> Result<Option<Document>, String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.read_tx
            .send(ReadCommand::Get { key, resp: resp_tx })
            .await
            .map_err(|_| "writer dropped".to_string())?;

        resp_rx.await.map_err(|_| "writer dropped".to_string())
    }

    pub async fn snapshot(&self) -> Result<(), String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.write_tx
            .send(WriteCommand::Snapshot { resp: resp_tx })
            .await
            .map_err(|_| "writer dropped".to_string())?;
        resp_rx.await.map_err(|_| "writer dropped".to_string())?
    }
}
