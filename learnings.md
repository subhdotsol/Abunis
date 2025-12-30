# STAGE 1 :  Why the Server Freezes and How to Prove It


Your server waits for the client to finish sending a complete HTTP request before it responds. If a client connects but sends nothing or sends only part of the request, the server blocks and waits forever. Because the server is single-threaded and uses blocking I/O, it can only handle one connection at a time, so while it is waiting on one client, all other clients are ignored.

This means a single slow or misbehaving client can freeze the entire server. When multiple clients connect, the first one that blocks prevents the server from accepting or responding to any others. This behavior is not a Rust bug or a TCP issue — it is a design limitation of a blocking, single-threaded server and explains why real servers rely on threads, async I/O, or timeouts.

You tested this behavior using these steps:

1. Start the server with `cargo run`.
2. Open many connections at once using `for i in {1..20}; do nc 127.0.0.1 8080 & done`, without sending any data.
3. Observe that the server accepts only one connection and then freezes, while all clients receive no response.
4. Try sending a valid HTTP request from another terminal and see that it also hangs, proving that one slow client can block the entire server.

---

# Stage 2 : Why the async server is better than the old blocking server

1. Handles multiple clients at once – each client runs in its own async task, so slow clients don’t block others.
2. Efficient waiting – uses async I/O, so one thread can handle many connections without sitting idle.
3. Independent clients – you can run multiple client programs (client.rs) simultaneously.
4. Scalable – can handle dozens or hundreds of clients without creating a thread per client.
5. Same functionality – still echoes the request body back correctly, just now concurrently.

---

# Stage 3 — Parse events (introduce correctness)

In Stage 3, the server moves from blindly handling bytes to enforcing correctness at the boundary. By fully reading request bodies, parsing JSON into a typed `Event`, and rejecting invalid input in a controlled way, the system becomes resilient to malformed, partial, or unexpected data. This stage proves that real-world clients cannot be trusted, that partial reads are normal, and that correctness must be designed explicitly rather than assumed. Establishing clear validation and error handling here creates a reliable contract on which all later stages—state management, concurrency, and performance—depend.

**Key learnings (points):**

* Real users and clients send malformed, incomplete, or garbage data.
* Network reads are partial by default; full bodies must be assembled before parsing.
* Parsing into a typed structure is both validation and documentation of the protocol.
* Errors should be handled explicitly and return predictable responses, not crashes.
* Async I/O improves waiting efficiency but does not guarantee correctness.
* Correctness at the boundary is the foundation for all future scalability and performance work.

---
# STEP 4 — Single-threaded processing (INTENTIONALLY SLOW)

What we did
- Parsed JSON into Event (like STEP 3)
- Introduced shared in-memory state:
- HashMap<String, i64> to store user balances
- Wrapped in Arc<Mutex<...>> for safe async access
- Updated state in the request handler:
- Added the event amount to the user’s balance

Added intentionally slow processing:
- sleep(Duration::from_millis(500)).await simulates heavy work
- Fixed async correctness:
- Used tokio::sync::Mutex instead of futures mutex
- Used tokio::time::sleep instead of blocking std::thread::sleep

## Key learnings

Async does not equal parallel:
- Multiple clients are accepted concurrently
- Slow work + mutex serializes requests

Shared state must be locked:
- Prevents race conditions
- Ensures balances are correct

Correctness from STEP 3 is preserved:
- Invalid/malformed JSON is rejected
- Server never crashes

Throughput is limited by slow processing:
- Single-threaded work is a bottleneck
- Simulates real-world “one slow client delays others” problem

---


# STEP 5 — Introduce bounded async channel (BACKPRESSURE PAIN) 

### **1. Decoupling IO from work**

By separating TCP handling from processing, you learned that user-facing paths must remain fast and predictable.

* Network IO should only **read, validate, and enqueue**
* Slow operations (sleep, DB writes, state mutation) belong elsewhere
* Mixing IO and work causes latency spikes and task pile-ups

This separation is the foundation of scalable systems.

---

### **2. Bounded queues define capacity**

The bounded channel forced you to explicitly declare how much work the system can hold.

* Queue size is a **hard capacity limit**, not a tuning hint
* Memory usage is now predictable
* Overload becomes visible instead of implicit

Capacity that isn’t explicit will be discovered the hard way.

---

### **3. Backpressure is honesty**

When the queue fills and `try_send` fails, the system refuses new work immediately.

* Clients get fast, clear rejection
* Latency stays bounded
* The server remains responsive

Backpressure shifts pain outward instead of letting it destroy the system internally.

---

### **4. Dropping work is sometimes correct**

Rejecting requests under load feels wrong, but it protects the system.

* Not all data is equally valuable
* Uptime is often more important than completeness
* Graceful loss beats catastrophic failure

Stable systems choose *which* failures they are willing to accept.

---

### **5. Queues smooth bursts, not overload**

The worker speed never changed, even under heavy load.

* Throughput is fixed by processing capacity
* Queues only absorb short spikes
* Sustained overload must result in rejection

This clarifies the difference between **latency control** and **capacity creation**.

---

### **6. Fail fast vs block is a policy decision**

Choosing `try_send` over `send().await` made overload explicit.

* Blocking hides pressure and spreads it through the system
* Failing fast keeps behavior predictable
* Clear rejection is easier to reason about than silent slowdown

Blocking is often just delayed failure.

---

### **7. Serialization can be a strength**

Using a single worker simplified correctness.

* One consumer means ordered processing
* State mutation is safe and understandable
* Throughput limits are intentional, not accidental

Concurrency is not always the right answer.

---

### **8. Stress testing reveals truth**

The client confirmed the system behaved as designed.

* Accepted requests matched queue capacity
* Rejections increased under load
* No crashes or undefined behavior

A system that fails *predictably* is a system you can trust.

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