use tokio::sync::mpsc;

use crate::{interface::Command, store::kv::Document};
use serde_json::Value;
use tokio::sync::oneshot;

pub struct HandlerEnum {
    tx: mpsc::Sender<Command>,
}

impl HandlerEnum {
    pub fn new(tx: mpsc::Sender<Command>) -> Self {
        Self { tx }
    }

    pub async fn set(&self, key: String, value: Value) -> Result<(), String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(Command::Set {
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
        self.tx
            .send(Command::Patch {
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
        self.tx
            .send(Command::Del { key, resp: resp_tx })
            .await
            .map_err(|_| "writer dropped".to_string())?;

        resp_rx.await.map_err(|_| "writer dropped".to_string())?
    }

    pub async fn get(&self, key: String) -> Result<Option<Document>, String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(Command::Get { key, resp: resp_tx })
            .await
            .map_err(|_| "writer dropped".to_string())?;

        resp_rx.await.map_err(|_| "writer dropped".to_string())
    }

    pub async fn snapshot(&self) -> Result<(), String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(Command::Snapshot { resp: resp_tx })
            .await
            .map_err(|_| "writer dropped".to_string())?;
        resp_rx.await.map_err(|_| "writer dropped".to_string())?
    }
}
