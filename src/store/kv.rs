use serde_json::Value;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

use crate::event::Event;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Document {
    pub value: serde_json::Value,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub data: HashMap<String, Document>, // store only holds the current state of the key the previous versions are only hold in wal file
}

impl Store {
    pub fn new() -> Self {
        Store {
            data: HashMap::new(),
        }
    }

    pub fn apply_event(&mut self, event: Event) {
        // this takes in event and mutate the state the memory
        match event.new {
            Value::Null => {
                self.data.remove(&event.key);
            }
            new_value => {
                self.data.insert(
                    // hashmap function
                    event.key,
                    Document {
                        value: new_value,
                        version: event.version,
                    },
                );
            }
        }
    }

    /*
     * Instead of mutating the state directly it should return the event and that event will go to the apply_event after it is written by WAL in a
     *WAL file and then it will be applid in the memory by apply_event
     */

    pub fn put(&self, key: String, value: Value) -> Event {
        let (previous_state, version) = self.previous_state_info(&key);

        let new_version = version;

        Event {
            key,
            old: previous_state,
            new: value,
            version: new_version,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Document> {
        self.data.get(key)
    }

    pub fn delete(&self, key: &str) -> Event {
        let (previous_state, version) = self.previous_state_info(&key);
        Event {
            key: key.to_string(),
            old: previous_state,
            new: Value::Null,
            version,
        }
    }

    pub fn patch(&self, key: &str, delta: Value) -> Event {
        let (previous_state, version) = self.previous_state_info(&key);

        // create merged value without mutating store

        let mut new_value = previous_state.clone(); // we are clonging to previous state to make changs to it

        merge_json(&mut new_value, &delta); // mutable borrow for the new_value
        Event {
            key: key.to_string(),
            old: previous_state,
            new: new_value,
            version,
        }
    }

    // Event struct must own the previous values so passing the reference to them and cloning them in the individual fuinction is sucha terible idea
    fn previous_state_info(&self, key: &str) -> (Value, u64) {
        match self.data.get(key) {
            Some(doc) => (doc.value.clone(), doc.version + 1),
            None => (Value::Null, 1),
        }
    }
}

fn merge_json(target: &mut Value, delta: &Value) {
    match (target, delta) {
        // If both are JSON objects → recursively merge fields
        (Value::Object(t), Value::Object(d)) => {
            for (k, v) in d {
                merge_json(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        // Otherwise, overwrite completely
        (t, d) => {
            *t = d.clone();
        }
    }
}

/*
The store most do one thing that is given an event compute the next event (state, event) -> next state

*/


// ? write → WAL first → apply_event second (basically call the operation here to give us new event), use that egvent to write to WAL , after writing to wal apply that event to return the new thing
/*
Database
   ├─ create Event
   ├─ wal.append(event)      ← durability
   └─ store.apply_event(event) ← memory update

*/