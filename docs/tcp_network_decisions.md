# FluxDB TCP Network Layer - Design Decisions

## Core Design Philosophy

FluxDB's network layer prioritizes **simplicity**, **isolation**, and **asynchronous streaming**. Each connection is independent, read/write paths are split, and subscriptions stream events without blocking regular commands.

---

## Decision 1: Line-Delimited JSON Protocol

### Decision
Use **newline-terminated JSON** for request/response framing.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Simplicity** | No complex framing protocol; just read until `\n` |
| **Debuggability** | Human-readable; can test with `netcat` |
| **Streaming** | Process requests as they arrive (no buffering full messages) |
| **Language agnostic** | Any language can parse JSON + newlines |

### Alternative Considered
**Length-prefixed binary protocol:**
```rust
// ❌ More complex: read 4 bytes for length, then read payload
let mut len_buf = [0u8; 4];
socket.read_exact(&mut len_buf).await?;
let len = u32::from_be_bytes(len_buf);
let mut payload = vec![0u8; len];
socket.read_exact(&mut payload).await?;
```
- ❌ Requires two reads per message
- ❌ Not human-readable
- ❌ Harder to debug with standard tools
- ✅ More efficient for binary data (rejected for simplicity)

### Implementation
```rust
// Server: Read line-delimited JSON
let mut line: String = String::new();
loop {
    line.clear();
    let n = reader.read_line(&mut line).await?;
    if n == 0 { break; } // EOF
    
    let req: Request = serde_json::from_str(line.trim())?;
    // Process request...
}

// Client: Send with newline terminator
let line = serde_json::to_string(&req)?;
write_half.write_all(line.as_bytes()).await?;
write_half.write_all(b"\n").await?;
```

---

## Decision 2: Per-Connection Task Model

### Decision
Spawn **one async task per TCP connection** with independent state.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Isolation** | One connection's failure doesn't affect others |
| **Simplicity** | No shared connection state to synchronize |
| **Scalability** | Tokio handles thousands of concurrent tasks |
| **Natural backpressure** | Each connection has own outbound buffer |

### Alternative Considered
**Connection pool with shared workers:**
```rust
// ❌ Complex: Route requests through shared worker pool
struct ConnectionPool {
    workers: Vec<Worker>,
    next_worker: AtomicUsize,
}
```
- ❌ Requires request routing logic
- ❌ Shared state needs synchronization
- ❌ Harder to track per-connection subscriptions
- ✅ Better resource utilization (rejected for simplicity)

### Implementation
```rust
// In server.rs main()
loop {
    let (stream, addr) = listener.accept().await?;
    stream.set_nodelay(true)?;
    let handle = handle.clone();
    
    // Each connection gets independent task
    tokio::spawn(async move {
        if let Err(e) = handle_connection(stream, handle).await {
            eprintln!("connection {addr} closed with error: {e}");
        }
    });
}
```

---

## Decision 3: Split Read/Write Halves

### Decision
Split TCP stream into **separate read and write halves**, each handled independently.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Full-duplex I/O** | Read and write can proceed concurrently |
| **Non-blocking writes** | Slow writes don't block request parsing |
| **Subscription streaming** | Events can be sent while reading new commands |
| **Tokio optimization** | `into_split()` enables efficient concurrent access |

### Alternative Considered
**Single stream with mutex:**
```rust
// ❌ BAD: Mutex on entire stream
struct Connection {
    stream: Arc<Mutex<TcpStream>>,
}

async fn send_response(&self, resp: Response) {
    let mut stream = self.stream.lock().await; // BLOCKS reads!
    stream.write_all(...).await;
}
```
- ❌ Write locks block read operations
- ❌ Defeats purpose of async I/O
- ✅ Simpler mental model (rejected for performance)

### Implementation
```rust
// Split stream once at connection start
let (read_half, mut write_half) = stream.into_split();

// Writer task owns write_half exclusively
let writer_task = tokio::spawn(async move {
    while let Some(resp) = out_rx.recv().await {
        let line_bytes = serde_json::to_vec(&resp).unwrap();
        write_half.write_all(&line_bytes).await.unwrap();
    }
});

// Read loop uses read_half independently
loop {
    let req = reader.read_line(&mut line).await?;
    // Process without blocking writes
}
```

---

## Decision 4: Outbound Channel Per Connection

### Decision
Each connection has a **bounded mpsc channel (128 messages)** for outbound responses.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Backpressure** | Slow clients block senders, preventing memory growth |
| **Decoupling** | Read loop doesn't wait for TCP write to complete |
| **Subscription isolation** | Multiple subscription tasks can send independently |
| **Bounded memory** | 128 messages × connections = predictable memory |

### Alternative Considered
**Direct write without channel:**
```rust
// ❌ BAD: Synchronous write in read loop
match req {
    Request::Set { key, value } => {
        let result = handle.set(key, value).await;
        // BLOCKS: Write directly to socket
        write_response(&mut write_half, result).await?;
    }
}
```
- ❌ Slow client blocks entire read loop
- ❌ Can't handle subscriptions concurrently
- ❌ No backpressure signal
- ✅ Simpler implementation (rejected for isolation)

### Implementation
```rust
const OUTBOUND_BUFFER: usize = 128;

// Create channel per connection
let (out_tx, mut out_rx) = mpsc::channel::<Response>(OUTBOUND_BUFFER);

// Writer task drains channel to socket
let writer_task = tokio::spawn(async move {
    while let Some(resp) = out_rx.recv().await {
        let line_bytes = serde_json::to_vec(&resp).unwrap();
        write_half.write_all(&line_bytes).await.unwrap();
    }
});

// Read loop sends to channel (non-blocking if not full)
let resp = Response::Ok;
let _ = out_tx.send(resp).await;
```

---

## Decision 5: Subscription Event Streaming

### Decision
Subscriptions spawn **independent tasks** that forward events to the outbound channel.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Non-blocking** | Event forwarding doesn't block read loop |
| **Multiple subscriptions** | Each key gets own forwarding task |
| **Natural lifecycle** | Task exits when client disconnects (channel closed) |
| **Shared outbound path** | Events use same channel as regular responses |

### Alternative Considered
**Read loop polls subscription:**
```rust
// ❌ BAD: Poll subscription in read loop
let mut sub_rx = handle.subscribe(key).await?;
loop {
    tokio::select! {
        req = reader.read_line() => { /* handle request */ }
        event = sub_rx.recv() => { /* send event */ }
    }
}
```
- ❌ Complex select logic for multiple subscriptions
- ❌ One subscription blocks others
- ❌ Hard to scale to multiple keys
- ✅ Simpler control flow (rejected for scalability)

### Implementation
```rust
// In server.rs - Subscribe handling
Request::Subscribe { key } => {
    match handle.subscribe(key.clone()).await {
        Ok(mut sub_rx) => {
            // Acknowledge subscription
            let _ = out_tx.send(Response::Subscribed { key }).await;
            
            // Spawn independent event forwarder
            let sub_tx = out_tx.clone();
            tokio::spawn(async move {
                while let Some(event) = sub_rx.recv().await {
                    if sub_tx.send(Response::Event { event }).await.is_err() {
                        break; // Client disconnected
                    }
                }
            });
        }
        Err(message) => {
            let _ = out_tx.send(Response::Error { message }).await;
        }
    }
}
```

---

## Decision 6: TCP_NODELAY Enabled

### Decision
Disable Nagle's algorithm with `set_nodelay(true)` for all connections.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Lower latency** | No 40ms delay for small packets |
| **Interactive use** | Shell commands respond immediately |
| **Line-delimited protocol** | Each request is one line; batching not beneficial |
| **Consistent behavior** | Predictable latency for benchmarks |

### Alternative Considered
**Default TCP buffering (Nagle's algorithm):**
- ❌ Small packets delayed up to 40ms
- ❌ Interactive shell feels sluggish
- ✅ Better network utilization for tiny packets (rejected for latency)

### Implementation
```rust
let (stream, addr) = listener.accept().await?;
stream.set_nodelay(true)?; // Disable Nagle's algorithm
```

---

## Decision 7: Shell Pending Lock (One In-Flight Request)

### Decision
Shell client uses a **Mutex-protected pending slot** to track one in-flight request at a time.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Response ordering** | Guarantees response matches command |
| **Simplicity** | No request IDs needed |
| **User experience** | Prevents command flooding |
| **Clear mental model** | One command → one response |

### Alternative Considered
**Request IDs for multiplexing:**
```rust
// ❌ More complex: Track multiple in-flight requests
struct RequestId(u64);
struct Pending {
    requests: DashMap<RequestId, oneshot::Sender<Response>>,
}
```
- ❌ Requires ID generation and tracking
- ❌ Complex cleanup on disconnect
- ✅ Allows pipelining (rejected for simplicity)

### Implementation
```rust
// In client.rs - run_shell()
let pending: Arc<Mutex<Option<oneshot::Sender<Response>>>> = Arc::new(Mutex::new(None));

// Before sending command
let (tx, rx) = oneshot::channel();
{
    let mut slot = pending.lock().await;
    if slot.is_some() {
        println!("request already in-flight, wait");
        continue;
    }
    *slot = Some(tx);
}

// Socket reader routes response
match resp {
    Response::Event { .. } => {
        let _ = event_tx.send(resp).await;
    }
    other => {
        if let Some(tx) = pending_for_reader.lock().await.take() {
            let _ = tx.send(other);
        }
    }
}
```

---

## Decision 8: Separate Event Channel (Shell)

### Decision
Shell client uses **separate channel for subscription events** vs command responses.

### Why

| Reason | Explanation |
| :--- | :--- |
| **UI separation** | Events printed asynchronously, don't block input |
| **Independent loops** | Event printer runs while user types commands |
| **Backpressure** | Full event channel drops old events (bounded) |
| **Clear labeling** | `[event]` prefix distinguishes from responses |

### Alternative Considered
**Single channel for all output:**
```rust
// ❌ BAD: Mix events and responses
loop {
    let resp = out_rx.recv().await?;
    match resp {
        Response::Event { .. } => println!("[event] {resp:?}"),
        other => println!("{resp:?}"),
    }
}
```
- ❌ Events block command responses
- ❌ Can't print events while waiting for response
- ✅ Simpler implementation (rejected for UX)

### Implementation
```rust
// In client.rs - run_shell()
let (event_tx, mut event_rx) = mpsc::channel::<Response>(128);

// Event printer task (independent loop)
tokio::spawn(async move {
    while let Some(ev) = event_rx.recv().await {
        println!("[event] {ev:?}");
    }
});

// Socket reader routes events separately
match resp {
    Response::Event { .. } => {
        let _ = event_tx.send(resp).await;
    }
    other => {
        if let Some(tx) = pending.lock().await.take() {
            let _ = tx.send(other);
        }
    }
}
```

---

## Decision 9: Clone EngineHandle Per Connection

### Decision
Each connection gets a **cloned EngineHandle** for engine communication.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Shared engine** | All connections use same database instance |
| **Channel semantics** | mpsc Sender is cheaply cloneable |
| **No synchronization** | Each handle is independent sender |
| **Natural cleanup** | Handle dropped when connection closes |

### Implementation
```rust
// In server.rs main()
let runtime = EngineRuntime::start();
let handle = runtime.handle;

loop {
    let (stream, addr) = listener.accept().await?;
    let handle = handle.clone(); // Cheap clone of Sender
    
    tokio::spawn(async move {
        handle_connection(stream, handle).await?;
    });
}
```

---

## Decision 10: Graceful Error Handling

### Decision
Errors are **logged and connection closed**, not propagated as panics.

### Why

| Reason | Explanation |
| :--- | :--- |
| **Server stability** | One bad client doesn't crash server |
| **Clear diagnostics** | Error logged with connection address |
| **Client isolation** | Other connections unaffected |

### Implementation
```rust
// In server.rs main()
tokio::spawn(async move {
    if let Err(e) = handle_connection(stream, handle).await {
        eprintln!("connection {addr} closed with error: {e}");
    }
});

// In handle_connection() - Invalid JSON
let req: Request = match serde_json::from_str(trimmed) {
    Ok(req) => req,
    Err(e) => {
        let _ = out_tx.send(Response::Error {
            message: format!("invalid request json: {e}"),
        }).await;
        continue; // Continue reading next request
    }
};
```

---

## Summary: Task Separation Rationale

| Task | Component | Why Separated |
| :--- | :--- | :--- |
| **Connection Accept** | Main loop | Sequential accept, spawn handlers |
| **Request Parsing** | Read loop | Independent of write operations |
| **Response Writing** | Writer task | Non-blocking; concurrent with reads |
| **Event Forwarding** | Subscription task | Multiple subscriptions per connection |
| **Socket Reading (Client)** | Socket reader task | Independent of stdin input |
| **Event Printing (Client)** | Event printer task | Async event display during input |

---

## Concurrency Guarantees

| Invariant | Enforcement Mechanism |
| :--- | :--- |
| **Per-connection isolation** | Separate task per connection |
| **Read-write concurrency** | Split TCP halves |
| **Response ordering** | Single outbound channel per connection |
| **Shell command ordering** | Pending lock ensures one in-flight |
| **Backpressure** | Bounded channels (128 messages) |
| **Graceful shutdown** | Task exits on channel close |

---

## References

- `src/bin/server.rs` - TCP server implementation
- `src/bin/client.rs` - CLI client implementation
- `src/net/protocol.rs` - Request/Response type definitions
- `src/engine/handler.rs` - EngineHandle API
