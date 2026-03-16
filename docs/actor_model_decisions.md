# FluxDB Actor Model - Design Decisions

## Core Design Philosophy

FluxDB's actor model is built on **separation of concerns** through message-passing concurrency. Each actor owns a specific responsibility and communicates via bounded channels, ensuring isolation, backpressure, and clear failure boundaries.

---

## Decision 1: Single-Writer Architecture

### Decision
All write operations flow through a **single Write Actor** that owns:
- WAL append
- Fsync batching
- Pending write queue
- Post-durability apply

### Why Separated

| Reason | Explanation |
|--------|-------------|
| **WAL ordering guarantee** | Single actor ensures total ordering of WAL entries without locks |
| **Fsync batching** | Multiple writes batched together before fsync improves throughput |
| **Durability barrier** | Writes wait in pending queue until fsync completes |
| **WAL-before-memory invariant** | Enforced naturally by actor's sequential execution |

### Alternative Considered
**Multiple writers with WAL locking:**
- ❌ Requires mutex on WAL segment
- ❌ Complex coordination for fsync batching
- ❌ Risk of deadlocks
- ✅ Higher write throughput (rejected for correctness)

### Implementation
```rust
// Single actor loop - serialized execution
loop {
    tokio::select! {
        res = rx.recv() => {
            // Handle write command
            handle_write_command(&mut db, &mut pending, cmd).await
        }
        _ = tick.tick() => {
            // Heartbeat for idle writes
        }
    }
    
    // Drain all pending messages
    while let Ok(cmd) = rx.try_recv() {
        handle_write_command(&mut db, &mut pending, cmd).await;
    }
    
    // Fsync batch
    if !pending.is_empty() {
        db.fsync_wal()?;
        // Apply + notify + ACK
        for p in pending.drain(..) { ... }
    }
}
```

---

## Decision 2: Read-Write Separation

### Decision
Read operations handled by a **separate Read Actor** with shared read-only access to the store.

### Why Separated

| Reason | Explanation |
|--------|-------------|
| **Concurrent read scaling** | Multiple reads can proceed in parallel |
| **Read-write lock separation** | RwLock allows concurrent reads, exclusive writes |
| **Independent backpressure** | Read channel pressure doesn't affect writes |
| **Future extensibility** | Can add read replicas without changing write path |

### Alternative Considered
**Single actor for reads and writes:**
- ❌ Reads block behind writes
- ❌ No concurrency benefit from RwLock
- ✅ Simpler implementation (rejected for performance)

### Implementation
```rust
// Read actor - minimal, non-blocking
pub async fn read_actor(
    mut read_rx: mpsc::Receiver<ReadCommand>,
    shared_store: Arc<RwLock<Store>>,
) {
    while let Some(cmd) = read_rx.recv().await {
        match cmd {
            ReadCommand::Get { key, resp } => {
                let guard = shared_store.read().await;
                let out = guard.get(&key).cloned();
                let _ = resp.send(out);
            }
        }
    }
}
```

---

## Decision 3: Two-Phase Snapshot (Key Separation)

### Decision
Snapshot operation is **split into two separate tasks**:

**Phase 1 - Snapshot Payload (Write Actor):**
- Acquires read lock on store
- Clones data + captures current LSN
- Returns `Snapshot { data, lsn }` struct
- **Fast, in-memory operation**

**Phase 2 - Snapshot Durability (Snapshot Actor):**
- Serializes snapshot to JSON
- Writes to temp file
- Fsync temp file
- Atomic rename
- Fsync directory
- **Slow, I/O bound operation**

### Why Separated

| Reason | Explanation |
|--------|-------------|
| **Non-blocking writes** | Write actor returns immediately after Phase 1 |
| **I/O isolation** | Disk writes happen in separate actor |
| **Clear responsibility** | Write actor owns store state; Snapshot actor owns file durability |
| **Independent scheduling** | Periodic snapshots don't interfere with write traffic |

### Alternative Considered
**Single actor handles entire snapshot:**
```rust
// ❌ BAD: Write actor does everything
WriteActor {
    // ... write operations ...
    
    // Blocks all writes during I/O!
    fn checkpoint(&mut self) {
        let snapshot = self.store.read().await.clone();
        // SLOW: File I/O blocks write actor
        write_snapshot_to_disk(&snapshot).await;
    }
}
```
- ❌ Write actor blocked during entire snapshot I/O
- ❌ New writes queue up waiting for snapshot to complete
- ✅ Simpler implementation (rejected for performance)

### Implementation

**Phase 1 - Write Actor:**
```rust
// In write_actor.rs
WriteCommand::Snapshot { resp } => {
    match db.checkpoint_payload().await {
        Ok(snapshot) => {
            // Fast: just clone data + LSN
            let _ = resp.send(Ok(snapshot));
        }
        Err(e) => {
            let _ = resp.send(Err(e.to_string()));
        }
    }
}

// In db.rs
pub async fn checkpoint_payload(&mut self) -> io::Result<Snapshot> {
    let lsn = self.wal.current_lsn()?;
    let guard = self.store.read().await;
    
    let snapshot = Snapshot {
        data: guard.data.clone(),
        lsn,
    };
    
    // Lock released, returns immediately
    Ok(snapshot)
}
```

**Phase 2 - Snapshot Actor:**
```rust
// In snapshot_actor.rs
async fn run_snapshot_cycle(
    write_tx: &mpsc::Sender<WriteCommand>
) -> Result<(), String> {
    // Phase 1: Request payload (fast)
    let snapshot = request_snapshot_payload(write_tx).await?;
    
    // Phase 2: Durability (slow, doesn't block write actor)
    checkpoint_durability("flux.wal", &snapshot)?;
    Ok(())
}

fn checkpoint_durability(wal_path: &str, snapshot: &Snapshot) -> Result<(), String> {
    let final_path = format!("{wal_path}.snapshot");
    let tmp_path = format!("{final_path}.tmp");
    
    // Atomic snapshot protocol
    let bytes = serde_json::to_vec(snapshot)?;
    let mut tmp_file = File::create(&tmp_path)?;
    tmp_file.write_all(&bytes)?;
    tmp_file.sync_all()?;           // Fsync temp
    rename(&tmp_path, &final_path)?; // Atomic rename
    File::open(dir)?.sync_all()?;    // Fsync directory
    Ok(())
}
```

---

## Decision 4: Reactive Dispatch Isolation

### Decision
Event dispatch to subscribers handled by a **separate Notify Actor**, not the Write Actor.

### Why Separated

| Reason | Explanation |
|--------|-------------|
| **Non-blocking dispatch** | Subscribers never block database writes |
| **Backpressure isolation** | Slow subscribers evicted without affecting write throughput |
| **Independent state** | Subscriber registry separate from store state |
| **Event ordering** | Single actor ensures per-key event ordering |

### Alternative Considered
**Write actor dispatches directly:**
```rust
// ❌ BAD: Write actor dispatches to subscribers
for p in pending.drain(..) {
    db.apply_event(p.event.clone());
    // SLOW: Dispatch to all subscribers blocks write actor
    for subscriber in subscribers {
        subscriber.send(p.event.clone()).await?;
    }
    let _ = p.resp.send(Ok(()));
}
```
- ❌ Slow subscribers block write actor
- ❌ Subscriber backpressure affects write throughput
- ✅ Simpler implementation (rejected for isolation)

### Implementation
```rust
// In write_actor.rs - just dispatch, don't wait
for p in pending.drain(..) {
    let event = p.event.clone();
    if let Err(e) = db.execute_post_durability(p.event).await {
        let _ = p.resp.send(Err(e.to_string()));
    } else {
        let _ = p.resp.send(Ok(()));
        // Non-blocking: send to notify actor, don't wait
        let _ = notify_tx.send(NotifyCommand::Dispatch { event }).await;
    }
}

// In notify_actor.rs - handles all subscriber management
pub async fn run(mut self) {
    while let Some(cmd) = self.rx.recv().await {
        match cmd {
            NotifyCommand::Subscribe { key, resp } => {
                let sub = self.reactivity.subscribe(&key);
                let _ = resp.send(sub);
            }
            NotifyCommand::Dispatch { event } => {
                // Non-blocking dispatch with eviction
                self.reactivity.dispatch_event(&event);
            }
        }
    }
}
```

**Backpressure Handling:**
```rust
// In Reactivity::dispatch_event
for (key, subscribers) in &mut self.subscribers {
    subscribers.retain(|tx| {
        // Non-blocking send
        match tx.try_send(event.clone()) {
            Ok(()) => true,           // Success, keep subscriber
            Err(TrySendError::Full(_)) => false, // Slow, evict
            Err(TrySendError::Closed) => false,  // Closed, remove
        }
    });
}
```

**Guarantee:** *Subscribers never block database writes.*

---

## Decision 5: Fsync Batching with Heartbeat

### Decision
Write actor uses a **5ms heartbeat timer** to batch fsync operations.

### Why

| Reason | Explanation |
|--------|-------------|
| **Throughput optimization** | Multiple writes batched into single fsync |
| **Latency bounding** | Heartbeat ensures idle writes don't wait indefinitely |
| **Opportunistic draining** | `try_recv()` drains all pending messages quickly |

### Implementation
```rust
let mut tick = interval(Duration::from_millis(5));
let mut pending: Vec<PendingWrite> = Vec::new();

loop {
    tokio::select! {
        // Wait for command OR heartbeat
        res = rx.recv() => { /* handle command */ }
        _ = tick.tick() => { /* heartbeat for idle writes */ }
    }
    
    // Opportunistically drain all available commands
    while let Ok(cmd) = rx.try_recv() {
        handle_write_command(&mut db, &mut pending, cmd).await;
    }
    
    // Fsync batch
    if !pending.is_empty() {
        db.fsync_wal()?;
        for p in pending.drain(..) { /* apply + notify + ACK */ }
    }
}
```

---

## Decision 6: Bounded Channels

### Decision
All actor channels use **bounded capacity of 32 messages**.

### Why

| Reason | Explanation |
|--------|-------------|
| **Backpressure** | Senders block when channel full, preventing unbounded growth |
| **Memory bounds** | System memory usage predictable under load |
| **Failure signaling** | Full channel indicates downstream actor overwhelmed |

### Alternative Considered
**Unbounded channels:**
- ❌ Memory can grow unbounded under load
- ❌ No backpressure signal
- ✅ Simpler, no blocking (rejected for safety)

---

## Decision 7: Arc<RwLock<Store>> for Shared State

### Decision
Shared store uses `Arc<RwLock<Store>>` for concurrent access.

### Why

| Component | Rationale |
|-----------|-----------|
| **Arc** | Reference counting for multiple actor ownership |
| **RwLock** | Concurrent reads, exclusive writes |

### Lock Granularity

| Actor | Lock Type | Duration |
|-------|-----------|----------|
| Write Actor | Write lock | Only during apply (post-fsync) |
| Read Actor | Read lock | Only during lookup |

**Never held during I/O operations.**

---

## Decision 8: Periodic + On-Demand Snapshots

### Decision
Snapshot actor triggers on **two conditions**:
1. **Periodic**: Every 30 seconds (timer-based)
2. **On-demand**: Every 1000 writes (write actor triggered)

### Why

| Reason | Explanation |
|--------|-------------|
| **Time-based** | Bounds WAL size even under low write traffic |
| **Write-based** | Bounds WAL size under high write traffic |
| **Dual trigger** | Covers both high and low traffic scenarios |

### Implementation
```rust
// In snapshot_actor.rs - periodic trigger
let mut tick = interval(Duration::from_secs(30));
loop {
    tokio::select! {
        _ = tick.tick() => {
            let _ = run_snapshot_cycle(&write_tx).await;
        }
        cmd = rx.recv() => { /* on-demand trigger */ }
    }
}

// In write_actor.rs - write-based trigger
writes_since_snapshot += 1;
if writes_since_snapshot >= SNAPSHOT_EVERY {
    let _ = snap_tx.send(SnapshotActorCommand::TriggerNow).await;
    writes_since_snapshot = 0;
}
```

---

## Concurrency Guarantees

| Invariant | Enforcement Mechanism |
|-----------|----------------------|
| **WAL-before-memory** | Write actor fsyncs before apply |
| **Single-writer serialization** | Only write actor can modify WAL |
| **Concurrent reads** | RwLock allows multiple readers |
| **Event ordering** | Notify actor dispatches in order |
| **Backpressure** | Bounded channels + slow subscriber eviction |
| **Crash recovery** | WAL replay from last snapshot LSN |

---

## Summary: Task Separation Rationale

| Task | Actor | Why Separated |
|------|-------|---------------|
| **Writes** | Write Actor | Single-writer serialization; WAL ordering; fsync batching |
| **Reads** | Read Actor | Concurrent read scaling; read-write lock separation |
| **Snapshot Payload** | Write Actor | Fast in-memory operation; owns store read lock |
| **Snapshot Durability** | Snapshot Actor | Slow I/O operation; shouldn't block writes; atomic file protocol |
| **Event Dispatch** | Notify Actor | Non-blocking dispatch; backpressure isolation; subscriber management |

---

## References

- `src/engine/runtime.rs` - Runtime initialization
- `src/engine/write_actor.rs` - Write actor implementation
- `src/engine/read_actor.rs` - Read actor implementation
- `src/engine/snapshot_actor.rs` - Snapshot actor implementation
- `src/engine/notify_actor.rs` - Notify actor implementation
- `src/engine/db.rs` - Database internal operations
- `src/store/kv.rs` - Store implementation
