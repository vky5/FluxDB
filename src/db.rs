use std::{fs::{File, rename}, io::{self, Write}};

use serde_json::Value;

use crate::{snapshot::Snapshot, store::{
    event::Event,
    kv::{Document, Store},
    wal::{Wal},
}};

pub struct Database {
    store: Store,
    wal: Wal,
}

impl Database {
    // Open DB + replay WAL (recovery)
    pub fn open(path: &str) -> io::Result<Self> {
        let mut wal = Wal::open(path)?;
        let mut store = Store::new();

        let snap_path = snapshot_path(&path);
        
        
        // ---- Try loading snapshot -----
        let offset = if let Ok(bytes) = std::fs::read(&snap_path) { 
            let snapshot:Snapshot = bincode::deserialize(&bytes).expect("invalid snapshot");

            store.data = snapshot.data;
            snapshot.wal_offset
        }else{
            0
        }; // ultimately returning wal offset and after writing store's data 



        // the next two steps are for recovery logic
        // replay persisted events
        // ------- recovery of wal suffix only
        let events = wal.replay_from(offset)?; // this restore the event to the previous state of it
        for event in events {
            store.apply_event(event);
        }

        Ok(Self { store, wal }) // this is a borrow and after that seek end and events writing that moves the seek to the end the cursor is at the end of the file which is fine because we want to write in the end anyway
    }

    // storing the latest value of the sotre in the checkpoint
    pub fn checkpoint(
        &mut self, 
        wal_path: &str,
    ) -> io::Result<()> {
        let path = snapshot_path(wal_path);

        let offset = self.wal.current_offset()?;

        let snapshot = Snapshot{
            data: self.store.data.clone(),
            wal_offset: offset,

        };

        let bytes = bincode::serialize(&snapshot)
            .expect("snapshot serialization must not fail");

        // write snapshot file
        let mut file = File::create(&path)?;
        file.write_all(&bytes)?;

        file.sync_all()?;

        Ok(())
    }

    pub fn truncate_wal(&mut self, wal_path: &str) -> io::Result<()> { // safe truncation requires knowledge of the global state of db like checkpoint timing that's why it doesnt belong to the wal file
        // ------ Get snapshot offset -------
        let snapshot_path = snapshot_path(wal_path);

        let bytes = std::fs::read(&snapshot_path)?;// we are reading not from the struct but because from the file that is already written because that is more durable
        let snapshot:Snapshot = bincode::deserialize(&bytes).expect("invalid snapshot");
        let offset = snapshot.wal_offset;


        // first get the records after that snapshot (To be written in new wal file)
        let suffix_events = self.wal.replay_from(offset)?;

        // ------ Craete a tmp file to write new wal ------
        let tmp_path = format!("{wal_path}.tmp");
        let mut tmp_file = File::create(&tmp_path)?;

        // record the events that we got from the old wal file after snapshot
        for event in &suffix_events {
            // serialize events
            let bytes = serde_json::to_vec(event).expect("event serialization must not fail");

            let len = bytes.len() as u32; 
            tmp_file.write_all(&len.to_be_bytes())?;
            tmp_file.write_all(&bytes)?;
        }
        // rename the file and fsync it and also the directory to store the metadata

        tmp_file.sync_all()?;
        rename(&tmp_path, &wal_path).expect("can not break during renaming");

        // fsync directory for rename durability
        let dir = std::path::Path::new(wal_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));

        File::open(dir)?.sync_all()?;

        self.wal = Wal::open(wal_path)?;




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