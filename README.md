# FluxDB — Stage 1

**FluxDB Stage-1** is a **single-node, durable, reactive key-value store** designed to teach and validate the **core invariants of real storage engines** before moving to concurrency and distributed consensus.

This stage focuses purely on:

* **correctness**
* **durability**
* **deterministic recovery**
* **bounded reactivity**

Not performance.
Not networking.
Not distribution.

---

# What Stage-1 Achieves

Stage-1 delivers a **fully crash-safe local database core** with:

### Durability

* Write-Ahead Log (WAL)
* `fsync` before visibility
* Redo-only crash recovery

### Persistence Acceleration

* Atomic snapshot creation
* WAL truncation after checkpoint
* Deterministic restart state

### Reactive System

* Key-level subscriptions
* Event dispatch on mutation
* Bounded channels with slow-subscriber eviction

### Correctness Invariants

* WAL-before-memory visibility
* Deterministic replay
* Silent recovery (no historical event emission)
* Bounded memory under slow consumers

These are **real database guarantees**, not demo features.

---

# Data Model

FluxDB stores:

```
key → JSON document + version
```

### Document Structure

```
{
  value: serde_json::Value,
  version: u64   // per-key logical version
}
```

Versioning is:

* **per-key**
* **monotonically increasing**
* foundation for **future replication & MVCC**

---

# Supported CLI Commands

Stage-1 exposes a **minimal terminal interface**.

## SET

```
SET <key> <json>
```

Stores or overwrites a JSON value.

**Examples**

```
SET a "hello"
SET b 42
SET user {"name":"vaibhav","age":21}
SET nums [1,2,3]
```

Behavior:

* WAL append → fsync → memory apply → event dispatch
* Version increments per mutation
* Invalid JSON is rejected

---

## GET

```
GET <key>
```

Returns the latest stored document.

**Example**

```
GET user
→ Document { value: {"name":"vaibhav","age":21}, version: 1 }
```

---

## DELETE

```
DELETE <key>
```

Removes the key via a **versioned tombstone event**.

Equivalent internal form:

```
new = null
```

Ensures:

* deterministic replay
* historical correctness in WAL

---

## PATCH

```
PATCH <key> <json_delta>
```

Performs a **recursive JSON merge**:

* object fields merged
* non-objects overwritten

**Example**

```
SET profile {"name":"vaibhav"}
PATCH profile {"age":21}
```

Result:

```
{"name":"vaibhav","age":21}
```

---

## CHECKPOINT

```
CHECKPOINT
```

Creates an **atomic snapshot** of:

* full in-memory state
* current WAL offset

Protocol:

```
write temp → fsync → atomic rename → fsync directory
```

Guarantees **crash-safe persistence**.

---

## EXIT

```
EXIT
```

Gracefully terminates the CLI.

---

# Storage Architecture

FluxDB Stage-1 uses **log-structured storage with checkpoints**.

## Write Path

```
Event
 → WAL append
 → fsync
 → apply to memory
 → reactive dispatch
```

## Recovery Path

```
Load snapshot
 → replay WAL suffix
 → rebuild deterministic state
```

Recovery is:

* **redo-only**
* **silent**
* **deterministic**

---

# Reactive Subsystem

Each key maintains:

```
multiple independent subscribers
```

Design:

```
key → Vec<bounded channels>
```

Backpressure handling:

* non-blocking dispatch (`try_send`)
* closed channel → removed
* full buffer → slow subscriber evicted

Guarantee:

> **Subscribers never block database writes.**

---

# What Stage-1 Does NOT Include

To preserve focus, Stage-1 intentionally excludes:

* networking / TCP server
* multi-process clients
* concurrency control
* async runtime integration
* indexing
* transactions / MVCC
* replication / Raft
* sharding
* distributed queries

All of these belong to **later stages**.

---

# Running FluxDB

```
cargo run
```

You will see:

```
Simple DB CLI. Commands: SET key json | GET key | DELETE key | PATCH key json | CHECKPOINT | EXIT
```

Enter commands interactively.

---

# Example Session

```
SET x {"y":"red"}
GET x
PATCH x {"z":"pink"}
GET x
CHECKPOINT
EXIT
```

After restart:

```
GET x
→ state restored from snapshot + WAL replay
```

This validates **durability and recovery correctness**.

---

# Engineering Significance of Stage-1

Completing Stage-1 means the system now has:

* **real crash safety**
* **deterministic state machine**
* **bounded reactive delivery**
* **atomic persistence protocol**

This crosses the boundary from:

```
toy CRUD project
```

to:

```
foundational storage engine core
```

---

# Next Stage

Stage-2 introduces:

* async runtime shell
* concurrent clients
* serialized command queue
* WAL fsync batching
* background snapshotting

Preparing FluxDB for:

```
Stage-3 → Raft replication
```

---

# License

Educational / experimental project.
