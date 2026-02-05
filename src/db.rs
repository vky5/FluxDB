use std::io;

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

        // replay persisted events
        let events = Wal::replay(path)?; // this restore the event to the previous state of it
        for event in events {
            store.apply_event(event);
        }

        Ok(Self { store, wal })
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
