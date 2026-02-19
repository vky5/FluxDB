
pub struct Lsn {
    segment: u64,
    offset: u64,
}

// lsn - log sequence number used to keep track of the position in the WAL for recovery and checkpointing
// it is a tuple of (segment, offset) where segment is the index of the WAL file and offset is the byte offset within that file
// for example, if we have wal files named wal_0, wal_1, wal_2, then the LSN (1, 100) would refer to the file wal_1 and the byte offset 100 within that file