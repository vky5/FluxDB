use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{event::Event, store::kv::Document};


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Set { key: String, value: Value },
    Get { key: String },
    Del { key: String },
    Patch { key: String, delta: Value },
    Snapshot,
    Subscribe { key: String },
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Value { doc: Option<Document> },
    Subscribed { key: String },
    Event { event: Event },
    Error { message: String },
}


/*
{
  "kind": "set",
  "key": "a",
  "value": 1
} 

instead of this 

{ "Set": { "key": "a", "value": 1 } }

and this is the contract between client and server.rs  the official communication protocol between the two 
*/