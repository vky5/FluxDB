# FluxDB Write-Ahead Logging (WAL) and LSN Theory

This document outlines the theoretical design and implementation of Write-Ahead Logging (WAL) and Log Sequence Numbers (LSN) in FluxDB.

## Log Sequence Number (LSN)

The **LSN** is a unique identifier for a specific position in the Write-Ahead Log. In FluxDB, it is implemented as a tuple of two 64-bit unsigned integers:

```rust
pub struct Lsn {
    pub segment: u64,
    pub offset: u64,
}
```

- **`segment`**: The ID of the WAL segment file (e.g., `0.log`, `1.log`).
- **`offset`**: The byte offset within that specific segment file where a record starts.

LSNs are used for:

1. **Recovery**: Determining which events have already been applied to the persistent store.
2. **Checkpointing**: Marking the point up to which the in-memory state has been safely flushed to a snapshot.
3. **Garbage Collection**: Identifying old WAL segments that are no longer needed for recovery.

## WAL Segmentation and Rotation

The WAL is not a single monolithic file. Instead, it is divided into multiple **segments** to manage disk space and facilitate garbage collection.

### Segmentation

- Each segment is a file named `{id}.log` located in the `wal/` directory.
- Records are appended sequentially to the "active" segment.
- Each record consists of a 4-byte length prefix (big-endian) followed by the JSON-serialized `Event` payload.

### Rotation

Rotation occurs when the active segment exceeds a predefined size limit (default: 64 MB).

1. **Flush**: The current active segment is `fsync`'d to ensure durability.
2. **New Segment**: A new segment file is created with an incremented ID (`active_segment_id + 1`).
3. **Activation**: The new segment becomes the "active" segment for future writes.

## Checkpoint Handling

A **checkpoint** is the process of persisting the current in-memory state (the `Store`) to a permanent snapshot file on disk.

### Process

1. **LSN Capture**: The current LSN (end of the current WAL) is recorded.
2. **Snapshot Creation**: The entire in-memory data map is serialized along with the captured LSN.
3. **Atomic Write**:
   - The snapshot is written to a temporary file (`flux.wal.snapshot.tmp`).
   - The temporary file is `fsync`'d.
   - The temporary file is atomically renamed to the final snapshot path (`flux.wal.snapshot`).
   - The parent directory is `fsync`'d to ensure the rename is durable.
4. **Garbage Collection**: Once the snapshot is safe, any WAL segments with an ID strictly less than the snapshot's `lsn.segment` can be safely deleted.

### Triggering

Checkpoints are currently triggered based on a simple heuristic: the number of writes since the last checkpoint. Once the `threshold_since_checkpoint` (default: 1000) is reached, a checkpoint is initiated.

## Recovery Mechanism

FluxDB uses a **Redo-only recovery** strategy combined with checkpoints.

1. **Load Snapshot**: On startup, the database attempts to read `flux.wal.snapshot`. If found, it populates the in-memory `Store` and retrieves the `start_lsn`.
2. **Replay WAL**: If no snapshot exists, it starts replaying from `Lsn::ZERO`. Otherwise, it starts from the `lsn` stored in the snapshot.
3. **Event Application**: The `WalIterator` opens the segment specified by the `start_lsn`, seeks to the `offset`, and begins reading events sequentially. All events found are re-applied to the `Store` to bring it to the most recent state.
4. **Resume Writes**: Once replay is complete, the WAL is ready for new appends starting from the current end of the log.
