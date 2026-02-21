# WalIterator Documentation

The `WalIterator` is a specialized component in FluxDB used to traverse the Write-Ahead Log (WAL) sequentially. It abstracts away the complexity of managing multiple segment files.

## Overview

During recovery or manual log inspection, the database needs to read events in the exact order they were written. Since the WAL is split into multiple `.log` segments, a simple file reader is insufficient. The `WalIterator` provides a unified stream of events across all segments.

## Key Responsibilities

- **Cross-Segment Traversal**: It transparently moves from one segment to the next (e.g., `1.log` to `2.log`) when it reaches the end of the current file.
- **LSN-Based Initiation**: It can start reading from any arbitrary position using a `Lsn` (segment ID + byte offset) via `Wal::replay_from(lsn)`.
- **Lazy Segment Loading**: It only opens a segment file when the previous one has been fully consumed, optimizing file descriptor usage.

## Replay Logic

The iteration process follows these steps:

1. **Initialization**: The iterator is initialized with a `current_segment_id`, a `last_segment_id` (the active segment at the time of creation), and the directory path.
2. **Event Reading**: When `next_event()` is called:
   - It attempts to read the next `Event` from the `current_segment`.
   - If a valid event is found, it is returned immediately.
3. **Segment Transition**:
   - If `read_next()` returns `None` (End of File), the iterator checks if `current_segment_id < last_segment_id`.
   - If more segments exist, it increments `current_segment_id`, opens the new segment file, and continues the loop to read the next event.
4. **Termination**:
   - If the `current_segment_id` reaches the `last_segment_id` and the final segment is exhausted, it returns `None`, signaling the end of the replay.

## Implementation Details

The iterator maintains its own `Segment` state, which includes the file handle and a buffer. It relies on the `Segment::read_next()` method to handle the low-level deserialization and length-prefix parsing.
