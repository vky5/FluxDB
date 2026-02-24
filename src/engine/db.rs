use std::{
    fs::{File, rename},
    io::{self, Write},
    sync::Arc,
};

use serde_json::Value;
use tokio::sync::{RwLock, mpsc};

use crate::reactivity::reactivity::Reactivity;
use crate::store::kv::Store;
use crate::store::snapshot::Snapshot;
use crate::store::wal::Wal;
use crate::{event::Event, store::wal::lsn::Lsn};

pub struct Database {
    store: Arc<RwLock<Store>>,
    wal: Wal,
    reactivity: Reactivity,
    write_since_checkpoint: u64,
    threshold_since_checkpoint: u64, // the number of writes after which checkpoint should be triggered, this is a simple heuristic and can be improved by considering the size of the wal or the time since last checkpoint etc
}

impl Database {
    // Open DB + replay WAL (recovery)
    pub async fn open(path: &str, store: Arc<RwLock<Store>>) -> io::Result<Self> {
        let wal = Wal::open(path, 64 * 1024 * 1024)?;
        let reactivity = Reactivity::new();

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
            reactivity,
            write_since_checkpoint: 0,
            threshold_since_checkpoint: 1000,
        }) // this is a borrow and after that seek end and events writing that moves the seek to the end the cursor is at the end of the file which is fine because we want to write in the end anyway
    }

    // storing the latest value of the sotre in the checkpoint
    pub async fn checkpoint(&mut self, wal_path: &str) -> io::Result<()> {
        let final_path = snapshot_path(wal_path);
        let tmp_path = format!("{final_path}.tmp");

        let lsn = self.wal.current_lsn()?;

        let guard = self.store.read().await;

        let snapshot = Snapshot {
            data: guard.data.clone(),
            lsn,
        };

        drop(guard);

        // serialize snapshot
        let bytes = serde_json::to_vec(&snapshot).expect("snapshot serialization must not fail");

        // --- 1. write to TEMP file ---
        let mut tmp_file = File::create(&tmp_path)?;
        tmp_file.write_all(&bytes)?;

        // --- 2. fsync TEMP file (durability of contents) ---
        tmp_file.sync_all()?;

        // --- 3. atomic rename TEMP → FINAL ---
        rename(&tmp_path, &final_path)?;

        // --- 4. fsync DIRECTORY (durability of rename metadata) ---
        let dir = std::path::Path::new(&final_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));

        File::open(dir)?.sync_all()?;

        self.wal.gc(lsn)?;

        Ok(())
    }

    pub fn subscribe(&mut self, key: &str) -> mpsc::Receiver<Event> {
        self.reactivity.subscribe(key)
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

        // 3. dispatch to subscribers
        self.reactivity.dispatch_event(&event);

        self.write_since_checkpoint += 1;
        if self.write_since_checkpoint >= self.threshold_since_checkpoint {
            self.checkpoint("flux.wal").await?;
            self.write_since_checkpoint = 0;
        }
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
        self.wal.active_segment.fsync()
    }
}

fn snapshot_path(wal_path: &str) -> String {
    format!("{wal_path}.snapshot")
}
