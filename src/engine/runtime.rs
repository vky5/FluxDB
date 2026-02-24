use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};

use crate::{
    engine::{handler::EngineHandle, read_actor::read_actor, write_actor::write_actor},
    interface::command::{ReadCommand, WriteCommand},
    store::kv::Store,
};

pub struct EngineRuntime {
    pub handle: EngineHandle,
}

impl EngineRuntime {
    pub fn start() -> Self {
        let (read_tx, read_rx) = mpsc::channel::<ReadCommand>(32);
        let (write_tx, write_rx) = mpsc::channel::<WriteCommand>(32); // channel for writing and updating, is generally slower.

        let shared_store = Arc::new(RwLock::new(Store::new()));

        tokio::spawn(read_actor(read_rx, shared_store.clone())); // cloned the pointer 
        tokio::spawn(write_actor(write_rx, shared_store)); // moved the ownership of shared_store 

        // in the end both pointing to same thing
        

        let handle = EngineHandle::new(read_tx, write_tx);

        Self { handle }
    }
}
