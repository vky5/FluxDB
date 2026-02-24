use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};

use crate::{interface::command::ReadCommand, store::kv::Store};

pub async fn read_actor(
    mut read_rx: mpsc::Receiver<ReadCommand>,
    shared_store: Arc<RwLock<Store>>,
) {
    while let Some(cmd) = read_rx.recv().await {
        match cmd {
            ReadCommand::Get { key, resp } => {
                let guard = shared_store.read().await;
                let out = guard.get(&key).cloned();
                let _ = resp.send(out);
            }
        }
    }
}
