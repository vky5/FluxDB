use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lsn {
    pub segment: u64,
    pub offset: u64,
}

impl Lsn {
    pub const ZERO: Lsn = Lsn {
        segment: 0,
        offset: 0,
    };

    pub fn new(segment: u64, offset: u64) -> Self {
        Self { segment, offset }
    }
}
// lsn - log sequence number used to keep track of the position in the WAL for recovery and checkpointing
// it is a tuple of (segment, offset) where segment is the index of the WAL file and offset is the byte offset within that file
// for example, if we have wal files named wal_0, wal_1, wal_2, then the LSN (1, 100) would refer to the file wal_1 and the byte offset 100 within that file