use std::{fs::File, io::{self, Write}};

use serde_json::Value;

use crate::store::{
    event::Event,
    kv::{Document, Store},
    wal::Wal,
};

pub struct Database {
    store: Store,
    wal: Wal,
}

impl Database {
    // Open DB + replay WAL (recovery)
    pub fn open(path: &str) -> io::Result<Self> {
        let wal = Wal::open(path)?;

        let mut store = Store::new();
        
        
        // ---- Try loading snapshot -----
        let snap_path = snapshot_path(path);
        if let Ok(bytes) = std::fs::read(&snap_path){
            let data: std::collections::HashMap<String, Document> = serde_json::from_slice(&bytes).expect("snapshot must be valid");

            store.data = data;
        }



        // the next two steps are for recovery logic
        // replay persisted events
        let events = Wal::replay(path)?; // this restore the event to the previous state of it
        for event in events {
            store.apply_event(event);
        }

        Ok(Self { store, wal })
    }

    // storing the latest value of the sotre in the checkpoint
    pub fn checkpoint(
        &self, 
        wal_path: &str,
    ) -> io::Result<()> {
        let path = snapshot_path(wal_path);

        let bytes = serde_json::to_vec(&self.store.data)
            .expect("snapshot serialization must not fail");

        // write snapshot file
        let mut file = File::create(&path)?;

        file.write_all(&bytes)?;

        file.sync_all()?;

        Ok(())
    }



    // PRIVATE write pipeline
    fn execute(&mut self, event: Event) -> io::Result<()> {
        // durability first
        self.wal.append(&event)?;

        // memory second
        self.store.apply_event(event);

        Ok(())
    }

    // Public safe write APIs
    pub fn put(&mut self, key: String, value: Value) -> io::Result<()> {
        let event = self.store.put(key, value);
        self.execute(event)
    }

    pub fn delete(&mut self, key: &str) -> io::Result<()> {
        let event = self.store.delete(key);
        self.execute(event)
    }

    pub fn patch(&mut self, key: &str, delta: Value) -> io::Result<()> {
        let event = self.store.patch(key, delta);
        self.execute(event)
    }

    // Read-only API
    pub fn get(&self, key: &str) -> Option<&Document> {
        self.store.get(key)
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