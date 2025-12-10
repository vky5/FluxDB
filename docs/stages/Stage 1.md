# Stage 1 — Local Reactive KV Store

Stage 1 is a local-only, in-memory key–value store with reactive subscriptions.  
There is no networking, persistence, or distributed logic in this stage.

## Goals
- Store JSON documents
- CRUD operations (put/get/delete/patch)
- Versioning for each document
- Subscriptions on keys
- Event dispatch when documents change
- Concurrency-safe API

## Milestones

### Milestone 1 — Core KV and Versioning
Features:
- In-memory storage using HashMap<String, Document>
- Document = { value: JSON, version: u64 }
- Operations:
  - put(key, value)
  - get(key)
  - delete(key)
- Version increments on every update

Outcome:
- Basic KV behavior validated in main.rs

---

### Milestone 2 — Patch / Partial Update
Features:
- patch(key, delta) merges JSON into existing document
- Recursive merge for nested objects
- Version increments after patch

Outcome:
- Update only changed fields without replacing whole document

---

### Milestone 3 — Subscriptions
Features:
- subscribe(key) -> returns an event stream
- Listeners are notified when:
  - put / delete / patch happens
- Event structure:
  - key
  - old value
  - new value
  - version
  - timestamp (optional)

Outcome:
- Clients can react to changes on specific keys

---

### Milestone 4 — Event Dispatch System
Features:
- Registry of active subscriptions:
  subscriptions: HashMap<String, Vec<mpsc::Sender<Event>>>
- On mutation:
  - broadcast event to all listeners
  - done asynchronously so writes do not block

Outcome:
- Reactive, concurrent event delivery

---

### Milestone 5 — Demo Program
Features:
- Show usage of the API:
  - create store
  - subscribe to a key
  - put value
  - patch value
  - delete key
- Print events to console

Outcome:
- Visual proof that Stage 1 works

---

## What Stage 1 does not include
To stay focused, this stage does not contain:
- networking
- persistence or files
- write-ahead log
- compaction
- sharding
- replication
- consensus
- gossip
- transactions
- indexes
- CRDTs
- distributed queries

Stage 1 is fully local and purely in memory.

## Summary
- JSON document store
- put/get/delete/patch
- versioning
- subscriptions and event dispatch
- concurrency-safe
- optional query API
- demo application
