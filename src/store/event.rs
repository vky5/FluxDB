use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub key: String,
    pub old: Value,
    pub new: Value,
    pub version: u64,
}


// this file is single source of truth. It is being written in the WAL and the 
// state is gonna be build by this so you need to be careful how u store the data into it 

