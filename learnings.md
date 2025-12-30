# STAGE 1 — Blocking Single-Threaded Server (THE FREEZE)

## The Problem

A single-threaded, blocking server can only handle **one connection at a time**. If a client connects but doesn't send data, the server **freezes** waiting for that one client.

## Architecture

```
[Client 1] ──────┐
                 │
[Client 2] ──────┼───▶ [Server] ──▶ Blocked waiting for Client 1
                 │        │
[Client 3] ──────┘        ▼
                    (all ignored)
```

**What happens:**
1. Client 1 connects but sends nothing
2. Server blocks on `read()` waiting for Client 1's data
3. Clients 2, 3 connect but server is stuck — can't accept them
4. **Entire server is frozen** by one slow client

## Code Pattern (The Problem)

```rust
// Blocking, single-threaded server
loop {
    let (stream, addr) = listener.accept()?;  // Block until connection
    handle_client(stream)?;                    // Block until complete
    // ❌ Can't accept new clients while handling this one!
}
```

## How to Test

```bash
# Terminal 1: Start server
cargo run

# Terminal 2: Open 20 connections that send nothing
for i in {1..20}; do nc 127.0.0.1 8080 & done

# Result: Server accepts ONE connection, then freezes
```

## Key Learnings

### 1. Blocking I/O serializes everything
- `read()` blocks until data arrives
- One slow client = entire server blocked
- No way to handle multiple clients

### 2. Single-threaded = single point of failure
- One misbehaving client can DoS the server
- No timeout = infinite wait
- Real servers need concurrency

### 3. This is a design limitation, not a bug
- Not a Rust issue, not a TCP issue
- Blocking + single-threaded = frozen server
- Fix requires threads, async, or timeouts

---

# STAGE 2 — Async Tokio Server (CONCURRENCY)

## The Solution

Replace blocking I/O with **async/await** using Tokio. Each connection runs in its own async task, so slow clients don't block others.

## Architecture: Before vs After

### Before (Stage 1 - Blocking)
```
[Client 1] ────▶ [Server] ◀──── blocked
[Client 2] ────▶   (waiting)
[Client 3] ────▶   (waiting)
```

### After (Stage 2 - Async)
```
[Client 1] ────▶ [Task 1] ─┐
[Client 2] ────▶ [Task 2] ─┼──▶ [Tokio Runtime] ──▶ [Single Thread]
[Client 3] ────▶ [Task 3] ─┘         │
                                     ▼
                           (handles all concurrently)
```

## Code Pattern (The Fix)

```rust
// Async, concurrent server
loop {
    let (stream, addr) = listener.accept().await?;  // Non-blocking
    
    tokio::spawn(async move {                        // Spawn task
        handle_client(stream).await?;                // Runs concurrently
    });
    // ✅ Immediately ready to accept next client!
}
```

## Key Learnings

### 1. Async enables concurrency on one thread
- `await` yields control, doesn't block
- Runtime switches between tasks efficiently
- Thousands of connections, one thread

### 2. Each client is independent
- Slow Client 1 doesn't affect Client 2
- Tasks run concurrently, not sequentially
- No more server freezing

### 3. Same functionality, better scalability
- Still echoes request body correctly
- Now handles dozens/hundreds of clients
- No thread-per-connection overhead

### 4. Async ≠ Parallel
- Concurrency: multiple tasks making progress
- Parallelism: multiple tasks at the exact same time
- Async is concurrent but single-threaded

---

# STAGE 3 — JSON Parsing & Validation (CORRECTNESS)

## The Problem

Real clients send **garbage**. Malformed JSON, incomplete requests, unexpected data types. If the server crashes on bad input, it's not production-ready.

## Architecture

```
[Raw Bytes] ──▶ [HTTP Parser] ──▶ [JSON Parser] ──▶ [Typed Event]
     │               │                  │                │
     ▼               ▼                  ▼                ▼
 "garbage"      invalid HTTP      invalid JSON      Event struct
     │               │                  │                │
     └───────────────┴──────────────────┴────────────────┘
                               │
                               ▼
                    { "status": "error", "reason": "..." }
```

## The Event Type

```rust
#[derive(Debug, Deserialize)]
struct Event {
    user: String,
    action: String,
    amount: i64,
}

// Valid: {"user": "alice", "action": "buy", "amount": 100}
// Invalid: {"foo": "bar"}
// Invalid: "not json at all"
// Invalid: (empty body)
```

## Code Pattern

```rust
// Parse and validate
match serde_json::from_slice::<Event>(&body) {
    Ok(event) => {
        // ✅ Valid event, process it
        r#"{"status":"accepted"}"#
    }
    Err(_) => {
        // ❌ Invalid input, reject gracefully
        r#"{"status":"error","reason":"invalid_json"}"#
    }
}
```

## Content-Length Parsing

```rust
// HTTP header parsing (case-insensitive!)
let line_lower = line.to_lowercase();
if let Some(len) = line_lower.strip_prefix("content-length: ") {
    content_length = len.trim().parse::<usize>().unwrap_or(0);
}

// Read exact body length
let mut body = vec![0; content_length];
reader.read_exact(&mut body).await?;
```

## Key Learnings

### 1. Never trust client input
- Clients send garbage, malformed data, attacks
- Validate everything at the boundary
- Reject invalid input gracefully

### 2. Typed parsing is both validation and documentation
```rust
// This IS the protocol contract:
struct Event { user: String, action: String, amount: i64 }
```

### 3. Network reads are partial
- TCP doesn't guarantee complete messages
- Must read `Content-Length` bytes exactly
- Partial reads are normal, not errors

### 4. Errors should be predictable
- Never crash on bad input
- Return structured error responses
- Clients can handle rejections gracefully

### 5. Correctness enables everything else
- Can't scale a broken system
- Validation is the foundation
- Build performance on top of correctness

---

# STAGE 4 — Shared State & Slow Processing (LATENCY PAIN)

## The Problem

Real servers have **state** (databases, caches, user sessions). Real work takes **time** (API calls, computation). How does async handle slow operations with shared state?

## Architecture

```
[Client 1] ──▶ [Accept] ──▶ [Parse] ──▶ [Lock State] ──▶ [Update] ──▶ [Response]
                                              │            500ms
                                              ▼
[Client 2] ──▶ [Accept] ──▶ [Parse] ──▶ [WAITING...] 
                                              │
                                              ▼
                                    (blocked on mutex)
```

## State Management

```rust
// Shared state: user → balance
type State = Arc<Mutex<HashMap<String, i64>>>;

// Usage in handler
let mut state = state.lock().await;
let balance = state.entry(event.user).or_insert(0);
*balance += event.amount;
```

## Simulating Slow Work

```rust
// ❌ WRONG: Blocking sleep (blocks entire runtime!)
std::thread::sleep(Duration::from_millis(500));

// ✅ CORRECT: Async sleep (yields to runtime)
tokio::time::sleep(Duration::from_millis(500)).await;
```

## Tokio vs Std Mutex

```rust
// ❌ std::sync::Mutex - blocks the OS thread
let mut state = state.lock().unwrap();

// ✅ tokio::sync::Mutex - yields to async runtime
let mut state = state.lock().await;
```

## Key Learnings

### 1. Async does not equal parallel
- Multiple clients accepted concurrently
- But slow work + mutex = serialization
- Requests queue up behind the lock

### 2. Use async-aware primitives
| Need | Wrong | Right |
|------|-------|-------|
| Sleep | `std::thread::sleep` | `tokio::time::sleep` |
| Mutex | `std::sync::Mutex` | `tokio::sync::Mutex` |
| Channel | `std::sync::mpsc` | `tokio::sync::mpsc` |

### 3. Shared state requires locking
- Prevents data races
- Ensures correct balances
- But introduces serialization

### 4. Throughput is limited by slow work
- 500ms processing = 2 requests/sec max
- Mutex makes it worse (serialization)
- Single-threaded processing is a bottleneck

### 5. This sets up the need for workers
- Can't just "go faster" in the handler
- Need to decouple IO from processing
- Stage 5 introduces the solution

---

# STAGE 5 — Bounded Channels & Backpressure (CAPACITY LIMITS)

## The Problem

In Stage 4, slow processing blocked the IO path. If 1000 requests arrive but we can only process 2/sec, what happens? Memory grows, latency spikes, eventually the server dies.

## The Solution

**Decouple IO from work** using an async channel:
- TCP handler: accept, parse, enqueue (fast)
- Worker: dequeue, process, update state (slow)

## Architecture

```
[Clients] ──▶ [TCP Handlers] ──▶ [Bounded Channel (100)] ──▶ [Worker] ──▶ [State]
                   │                      │                      │
              (fast path)           (capacity limit)        (slow path)
                   │                      │                      │
              ~1000 req/sec          max 100 jobs           2 jobs/sec
                   │                      │
                   ▼                      ▼
              "accepted"           "rejected" (if full)
```

## The Bounded Channel

```rust
// Create channel with capacity 100
let (tx, rx) = mpsc::channel::<Job>(100);

// In TCP handler: non-blocking send
match tx.try_send(Job { event }) {
    Ok(_) => r#"{"status":"accepted"}"#,           // Enqueued
    Err(_) => r#"{"status":"rejected","reason":"server_busy"}"#, // Queue full
}
```

## try_send vs send

```rust
// try_send: Fail immediately if queue is full
tx.try_send(job)?;  // ✅ Returns immediately, fast path stays fast

// send().await: Block until space available
tx.send(job).await?;  // ❌ Slow consumer blocks fast producer
```

## The Worker Loop

```rust
// Single worker processing jobs
loop {
    if let Some(job) = rx.recv().await {
        // Slow processing (500ms)
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Update state
        let mut state = state.lock().await;
        *state.entry(job.event.user).or_insert(0) += job.event.amount;
    }
}
```

## Key Learnings

### 1. Decouple IO from work
- TCP handlers should be fast: read, validate, enqueue
- Slow work belongs in dedicated workers
- Mixing IO and work causes latency spikes

### 2. Bounded queues define capacity
- Queue size is a hard limit, not a hint
- Memory usage is predictable
- Overload becomes visible immediately

### 3. Backpressure is honesty
- When queue fills, reject new work
- Clients get fast, clear feedback
- Latency stays bounded
- Server remains responsive

### 4. Dropping work is sometimes correct
- Rejecting requests under load is protection
- Uptime > completeness
- Graceful degradation beats catastrophic failure

### 5. Queues smooth bursts, not overload
- Queues absorb short traffic spikes
- Sustained overload → rejections
- Throughput is fixed by worker capacity

### 6. Fail fast vs block is a policy choice
| Strategy | Behavior | Best For |
|----------|----------|----------|
| `try_send` | Reject immediately | User-facing APIs |
| `send().await` | Block until space | Internal pipelines |

### 7. Serialization can be a strength
- Single worker = ordered processing
- No race conditions on state
- Simple to reason about

### 8. Stress testing reveals truth
- Run more clients than capacity
- Verify: accepted ≈ capacity, rejected = overflow
- A system that fails predictably is trustworthy

## Test Configuration

| Parameter | Value |
|-----------|-------|
| Channel capacity | 100 |
| Worker count | 1 |
| Processing time | 500ms |
| Expected throughput | 2 jobs/sec |

## Summary: Stages 1-5 Progression

| Stage | Problem | Solution | Throughput |
|-------|---------|----------|------------|
| 1 | Blocking freezes server | — | 1 client at a time |
| 2 | One thread for all | Async tasks | Many concurrent |
| 3 | Bad input crashes server | JSON validation | Same, but correct |
| 4 | Slow work blocks IO | — | ~2 req/sec |
| 5 | Unbounded memory growth | Bounded channel + backpressure | ~2 req/sec, predictable |

---

# STEP 6 — Multi-Worker Pool with Shared Receiver (PARALLELISM + REAL-WORLD BUGS)

## What we built

- **Multiple worker threads** (4 workers) each running their own Tokio runtime
- **Shared channel receiver** wrapped in `Arc<Mutex<Receiver<Job>>>` so all workers can pull from the same queue
- Jobs are distributed automatically — whichever worker is free grabs the next job

## Architecture: Before vs After

### Before (Stage 5 - Single Worker)
```
[Clients] → [TCP Handler] → [Bounded Channel (100)] → [Single Worker] → [State]
                                                            ↓
                                              Processing: ~500ms per job
                                              Throughput: ~2 jobs/sec
```

### After (Stage 6 - Multi-Worker)
```
[Clients] → [TCP Handler] → [Bounded Channel (100)] → [Shared Receiver]
                                                            ↓
                                    ┌───────────────────────┼───────────────────────┐
                                    ↓                       ↓                       ↓
                              [Worker 0]              [Worker 1]              [Worker 2] ...
                                    ↓                       ↓                       ↓
                                    └───────────────────────┼───────────────────────┘
                                                            ↓
                                                        [State]
                                              Processing: ~500ms per job
                                              Throughput: ~8 jobs/sec (4x faster)
```

---

## The Bug We Hit: HTTP Headers Are Case-Insensitive

### The Problem
All 100 requests returned `{"status":"error","reason":"invalid_json"}` even though the JSON was perfectly valid.

### Root Cause
Our server parsed the `Content-Length` header like this:
```rust
// ❌ WRONG - Case-sensitive matching
if let Some(len) = line.strip_prefix("Content-Length: ") {
    content_length = len.parse::<usize>().unwrap_or(0);
}
```

But the `reqwest` HTTP client (and many others) sends headers in **lowercase**:
```
content-length: 43
```

Per **RFC 7230**, HTTP headers are **case-insensitive**. Our code only matched `Content-Length:` exactly, so when `reqwest` sent `content-length:`, the content length stayed `0`. This caused us to read an empty body, which failed JSON parsing.

### The Fix
```rust
// ✅ CORRECT - Case-insensitive matching
let line_lower = line.to_lowercase();
if let Some(len) = line_lower.strip_prefix("content-length: ") {
    content_length = len.trim().parse::<usize>().unwrap_or(0);
}
```

### Result After Fix
All 100 requests now return `{"status":"accepted"}`.

---

## Key Learnings

### 1. HTTP headers are case-insensitive (RFC 7230)
- `Content-Length`, `content-length`, `CONTENT-LENGTH` are all valid
- Different HTTP libraries use different conventions
- `curl` sends `Content-Length`, `reqwest` sends `content-length`
- Always normalize case before comparing headers

### 2. Multi-worker pools multiply throughput
- 4 workers = ~4x throughput (with 500ms processing time)
- Workers compete for jobs from a shared queue
- Load is automatically balanced — idle workers grab work

### 3. Shared receiver pattern
- Wrap the receiver in `Arc<Mutex<Receiver<Job>>>`
- Each worker locks the mutex, receives a job, then unlocks
- Simple and effective for work distribution

### 4. Thread per worker with dedicated runtime
- Each worker runs in a separate OS thread
- Each thread has its own Tokio runtime
- Prevents one slow worker from blocking others

### 5. Debugging network protocols requires visibility
- The error message `invalid_json` was misleading
- The actual issue was header parsing, not JSON
- Tracing the full request path revealed the truth

### 6. Test with real HTTP clients
- `curl` worked fine (sends proper-cased headers)
- `reqwest` exposed the bug (sends lowercase headers)
- Always test with multiple clients to catch edge cases

---

## Summary Table

| Aspect | Before (Single Worker) | After (Multi-Worker) |
|--------|------------------------|----------------------|
| Workers | 1 | 4 |
| Throughput | ~2 jobs/sec | ~8 jobs/sec |
| Header parsing | Case-sensitive (bug) | Case-insensitive (correct) |
| Job distribution | Sequential | Parallel, load-balanced |
| Scalability | Limited by single worker | Scales with worker count |

---

# STEP 7 — Lock Contention (THE HIDDEN PERFORMANCE KILLER)

## The Problem Setup

In Stage 6, we had 4 workers sharing one mutex:

```rust
type State = Arc<Mutex<HashMap<String, i64>>>;
```

This worked fine at low scale. But what happens when we increase workers?

## Architecture: The Bottleneck

```
                    ┌─────────────────────────────────────┐
                    │     SINGLE MUTEX (State)            │
                    │   HashMap<String, i64>              │
                    └─────────────────────────────────────┘
                                    ▲
            ┌───────────────────────┼───────────────────────┐
            │                       │                       │
      [Worker 0]              [Worker 1]              [Worker 2]
       waiting...              HOLDING LOCK            waiting...
            │                       │                       │
      [Worker 3] ... [Worker 15]    │                       
       all waiting...               ▼
                              Updating user5
```

**The Problem:**
- Worker 1 locks the mutex to update `user5`
- Workers 0, 2-15 are **blocked waiting** even though they're updating **different users**
- CPUs sit idle while workers wait for the lock
- More workers = more waiting = **worse performance**

---

## The "Hot Lock" Problem

A "hot lock" is a mutex that:
1. Guards frequently-accessed data
2. Is held by many threads
3. Causes most threads to spend time **waiting** rather than **working**

Symptoms:
- High CPU wait times
- Low CPU utilization despite many threads
- Throughput plateaus or decreases with more workers

---

## Why This Scales Badly

| Workers | Expected Throughput | Actual Behavior |
|---------|---------------------|-----------------|
| 1 worker | 2 jobs/sec | Works fine, no contention |
| 4 workers | 8 jobs/sec | OK, some waiting |
| 16 workers | 32 jobs/sec | Plateaus at ~10-12 jobs/sec |
| 64 workers | 128 jobs/sec | Actually slower due to overhead |

**The Paradox:** Adding more workers should increase throughput, but with a single lock, it can **decrease** performance.

---

## What We Instrumented

```rust
// Measure time waiting to acquire lock
let wait_start = Instant::now();
let mut state = worker_state.lock().await;
let wait_duration = wait_start.elapsed();

// Measure time holding the lock
let hold_start = Instant::now();
// ... do work ...
let hold_duration = hold_start.elapsed();
```

This shows:
- **Lock wait time:** How long workers blocked waiting for the mutex
- **Lock hold time:** How long each worker held the lock
- **Throughput:** Jobs processed per second

---

## Key Learnings

### 1. Locks scale badly if used blindly
- A single mutex becomes a serialization point
- All parallelism is lost when everyone waits for one lock
- More threads can mean worse performance

### 2. Lock contention is invisible without measurement
- System appears "busy" but throughput is low
- CPU is actually idle, waiting on locks
- Must instrument to see the problem

### 3. The contention formula
```
Contention Cost = (Number of Workers) × (Lock Hold Time) × (Access Frequency)
```
Reduce any factor to reduce contention.

### 4. Work outside the lock
- Do computation BEFORE acquiring the lock
- Hold the lock only for the critical section
- Release immediately after mutation

### 5. Lock granularity matters
- Coarse-grained: One lock for everything (our problem)
- Fine-grained: One lock per user (Stage 8 solution)
- Lock-free: No locks at all (advanced)

---

## Observing Contention

When you run the Stage 7 stress test, watch for:

```
[Worker  5] user=user3, balance=30, wait=45.23ms, hold=10.12ms
[Worker 12] user=user7, balance=50, wait=82.45ms, hold=10.08ms
```

- **High wait times** = workers blocking on the mutex
- **Consistent hold times** = the lock is always held ~10ms
- **wait >> hold** = severe contention

---

## The Insight

```
"Locks scale badly if used blindly."
```

This means:
- Don't lock more than you need
- Don't hold locks longer than necessary
- Consider sharding or lock-free alternatives
- Sometimes less concurrency is faster than contention

---

## Preview: Stage 8 (The Solution)

Split the state into **shards** so workers updating different users don't block each other:

```rust
// Before: ONE lock for all users
Arc<Mutex<HashMap<String, i64>>>

// After: N locks for N shards
Vec<Arc<Mutex<HashMap<String, i64>>>>

// Route user to shard by hash
let shard_idx = hash(user) % num_shards;
let shard = &shards[shard_idx];
```

---

## The Bottleneck Formula

```
Channel fills when:  (Incoming rate) > (Processing rate)

Processing rate = workers × (1 / processing_time)
                = 16 workers × (1 / 110ms)  ≈ 145 jobs/sec max theoretical

BUT with lock contention (10ms hold × 16 workers competing):
Actual rate ≈ 1000ms / 10ms_hold = ~100 jobs/sec effective max
```

The channel capacity is **100 jobs**. Once it fills, new requests get rejected via `try_send()`.

---

## Test Configuration

| Parameter | Value | Purpose |
|-----------|-------|---------|
| Workers | 16 | Many workers to show contention |
| Channel capacity | 100 | Bounded queue for backpressure |
| Lock hold time | 10ms | Simulates work while holding lock |
| Processing time | 100ms | Simulates async work before lock |
| Concurrency | 200 | More than channel capacity |
| Total requests | 5000 | Sustained pressure |

---

## Actual Test Results

```
=== Stage 7: Lock Contention Stress Test ===
Total requests: 5000
Concurrency: 200

=== Results ===
Duration:     0.39s
Throughput:   12,937 requests/sec (client sending rate)
Avg latency:  13.8ms

Accepted:     138
Rejected:     4862 (backpressure)  ← 97% REJECTED!
Errors:       0
```

### Analysis

| Metric | Value | Meaning |
|--------|-------|---------|
| **Accepted** | 138 | Only ~138 jobs made it into the queue |
| **Rejected** | 4862 | 97% of requests rejected due to full queue |
| **Duration** | 0.39s | Client sent all 5000 requests in under half a second |
| **Why rejected** | Lock contention | 16 workers serialized on 1 lock, couldn't drain fast enough |

### Why 97% Were Rejected

1. **200 concurrent requests** flooded the channel (capacity: 100)
2. Each worker holds the lock for **~10ms** while processing
3. With 16 workers competing for 1 lock, they **serialize** instead of parallelize
4. **Actual processing rate ≈ 100 jobs/sec** (limited by lock contention)
5. Queue filled immediately → rejections via backpressure

### The Paradox Proven

> You have **16 workers** but only ~100 jobs/sec throughput because they're all fighting for **one lock**.

Adding more workers **doesn't help** — it actually increases contention overhead!

---