use std::{
    io::{self},
    sync::Arc,
};

use serde_json::Value;
use tokio::sync::RwLock;

use crate::store::kv::Store;
use crate::store::snapshot::Snapshot;
use crate::store::wal::Wal;
use crate::{event::Event, store::wal::lsn::Lsn};

pub struct Database {
    store: Arc<RwLock<Store>>,
    wal: Wal,
    pub fail_next_fsync: bool,
}

impl Database {
    // Open DB + replay WAL (recovery)
    pub async fn open(path: &str, store: Arc<RwLock<Store>>) -> io::Result<Self> {
        let wal = Wal::open(path, 64 * 1024 * 1024)?;

        let snap_path = snapshot_path(&path);

        let mut guard = store.write().await; // taking exclusive write lock
        *guard = Store::new(); // replacing the entire guard value

        let start_lsn = if let Ok(bytes) = std::fs::read(&snap_path) {
            let snapshot: Snapshot = serde_json::from_slice(&bytes).expect("invalid snapshot");

            guard.data = snapshot.data;
            snapshot.lsn
        } else {
            Lsn::ZERO
        };

        let mut iter = wal.replay_from(start_lsn)?;
        while let Some(event) = iter.next_event()? {
            guard.apply_event(event);
        }

        drop(guard); // usually the lock is realased automatically when the scope ends but can use exclusively 

        Ok(Self {
            store,
            wal,
            fail_next_fsync: false,
        })
    }

    // storing the latest value of the sotre in the checkpoint
    pub async fn checkpoint_payload(&mut self) -> io::Result<Snapshot> {
        let lsn = self.wal.current_lsn()?;

        let guard = self.store.read().await;

        let snapshot = Snapshot {
            data: guard.data.clone(),
            lsn,
        };

        drop(guard);
        Ok(snapshot)
    }

    // PRIVATE write pipeline
    fn execute_pre_durability(&mut self, event: Event) -> io::Result<Event> {
        // 1. WAL durability
        self.wal.append(&event)?;
        Ok(event)
    }

    pub async fn execute_post_durability(&mut self, event: Event) -> io::Result<()> {
        // 2. apply to memory (shared store)
        {
            let mut guard = self.store.write().await;
            guard.apply_event(event.clone());
        } // write lock released here
        Ok(())
    }

    // Public safe write APIs
    pub async fn put(&mut self, key: String, value: Value) -> io::Result<Event> {
        let guard = self.store.read().await;
        let event = guard.put(key, value);
        drop(guard);
        self.execute_pre_durability(event)
    }

    pub async fn delete(&mut self, key: &str) -> io::Result<Event> {
        let guard = self.store.read().await;
        let event = guard.delete(key);
        drop(guard);
        self.execute_pre_durability(event)
    }

    pub async fn patch(&mut self, key: &str, delta: Value) -> io::Result<Event> {
        let guard = self.store.read().await;
        let event = guard.patch(key, delta);
        drop(guard);
        self.execute_pre_durability(event)
    }

    pub fn fsync_wal(&mut self) -> io::Result<()> {
        if self.fail_next_fsync {
            self.fail_next_fsync = false;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "injected fsync failure",
            ));
        }
        self.wal.active_segment.fsync()
    }
}

fn snapshot_path(wal_path: &str) -> String {
    format!("{wal_path}.snapshot")
}
