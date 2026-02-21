use std::{
    fs::{File, rename},
    io::{self, Write},
};

use serde_json::Value;
use tokio::sync::mpsc;

use crate::reactivity::reactivity::Reactivity;
use crate::store::kv::{Document, Store};
use crate::store::snapshot::Snapshot;
use crate::store::wal::Wal;
use crate::{event::Event, store::wal::lsn::Lsn};

pub struct Database {
    store: Store,
    wal: Wal,
    reactivity: Reactivity,
    write_since_checkpoint: u64,
    threshold_since_checkpoint: u64, // the number of writes after which checkpoint should be triggered, this is a simple heuristic and can be improved by considering the size of the wal or the time since last checkpoint etc
}

impl Database {
    // Open DB + replay WAL (recovery)
    pub fn open(path: &str) -> io::Result<Self> {
        let wal = Wal::open(path, 64 * 1024 * 1024)?;
        let mut store = Store::new();
        let reactivity = Reactivity::new();

        let snap_path = snapshot_path(&path);

        let start_lsn = if let Ok(bytes) = std::fs::read(&snap_path) {
            let snapshot: Snapshot = serde_json::from_slice(&bytes).expect("invalid snapshot");

            store.data = snapshot.data;
            snapshot.lsn
        } else {
            Lsn::ZERO
        };

        let mut iter = wal.replay_from(start_lsn)?;
        while let Some(event) = iter.next_event()? {
            store.apply_event(event);
        }

        Ok(Self {
            store,
            wal,
            reactivity,
            write_since_checkpoint: 0,
            threshold_since_checkpoint: 1000,
        }) // this is a borrow and after that seek end and events writing that moves the seek to the end the cursor is at the end of the file which is fine because we want to write in the end anyway
    }

    // storing the latest value of the sotre in the checkpoint
    pub fn checkpoint(&mut self, wal_path: &str) -> io::Result<()> {
        let final_path = snapshot_path(wal_path);
        let tmp_path = format!("{final_path}.tmp");

        let lsn = self.wal.current_lsn()?;

        let snapshot = Snapshot {
            data: self.store.data.clone(),
            lsn,
        };

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

    pub fn execute_post_durability(&mut self, event: Event) -> io::Result<()> {
        // 2. apply to memory
        self.store.apply_event(event.clone());

        // 3. dispatch to subscribers
        self.reactivity.dispatch_event(&event);

        self.write_since_checkpoint+=1;
        if self.write_since_checkpoint >= self.threshold_since_checkpoint {
            self.checkpoint("flux.wal")?;
            self.write_since_checkpoint = 0;
        }
        Ok(())
    }

    // Public safe write APIs
    pub fn put(&mut self, key: String, value: Value) -> io::Result<Event> {
        let event = self.store.put(key, value);
        self.execute_pre_durability(event)
    }

    pub fn delete(&mut self, key: &str) -> io::Result<Event> {
        let event = self.store.delete(key);
        self.execute_pre_durability(event)
    }

    pub fn patch(&mut self, key: &str, delta: Value) -> io::Result<Event> {
        let event = self.store.patch(key, delta);
        self.execute_pre_durability(event)
    }

    // Read-only API
    pub fn get(&self, key: &str) -> Option<&Document> {
        self.store.get(key)
    }

    pub fn fsync_wal(&mut self) -> io::Result<()> {
        self.wal.active_segment.fsync()
    }
}

fn snapshot_path(wal_path: &str) -> String {
    format!("{wal_path}.snapshot")
}

/*
this architecture is called Log Structured storage with checkpointing

for now we are replaying entire wal file even after recovering from snapshot but later we will

and trhis recovery method is called redo only recovery with checpoints

and why it is called redo only because we implementing the replay logic entirely in here


? why wal is still replayed after snapshot
- Snapshot covers state up to LSN (Last Sequenece Number)
- Snapshot implicitly represents a prefix of the log

*/
