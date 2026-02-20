// this is the main file to hanle the sapshot related logic

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::store::wal::lsn::Lsn;
use crate::store::kv::Document;

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub data: HashMap<String, Document>,
    pub lsn: Lsn,
}
