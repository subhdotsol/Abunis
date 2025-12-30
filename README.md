# ANUBIS - High-Performance Async Event Processing Engine

A learning-focused but production-style backend system that ingests a large number of real-time events, applies backpressure, processes them in parallel, and maintains correct shared state under load.

Think of it as a **mini trading engine / mini Solana validator / log ingestion service** — built to *feel* concurrency, not just read about it.

---

## 🚀 Why this project exists

Most people learn async, threads, mutexes, and channels **in isolation** and still don’t understand how real systems work.

This project exists to answer one question:

> **How do you safely and efficiently process massive real-time traffic without crashing or corrupting data?**

By building *one* system that touches everything.

---

## 🧠 What a user sees (plain English)

* A client connects over TCP
* Sends JSON events
* Gets an immediate response:

  * ✅ accepted
  * ❌ rejected (server busy)

The server:

* stays fast
* never blocks
* never corrupts state
* survives traffic spikes

---

## 📦 Example event

```json
{"user":"alice","action":"buy","amount":100}
```

---

## 🏗️ System architecture

```
TCP Clients
   ↓
Async TCP Server (Tokio)
   ↓
Bounded Async Channel  ← backpressure
   ↓
Worker Pool (OS Threads)
   ↓
Shared State (Arc + Lock)
   ↓
Metrics (Atomics)
```

---

## 🔧 Tech stack

* **Rust** — correctness & performance
* **Tokio** — async networking
* **Crossbeam** — thread channels
* **Arc / Mutex / RwLock** — shared state
* **Atomics** — lock-free metrics
* **Serde** — JSON parsing

---

## 📁 Project structure

```
src/
├── main.rs        # bootstrap + shutdown
├── net.rs         # async TCP server
├── pipeline.rs    # channels + backpressure
├── worker.rs      # worker threads
├── state.rs       # shared state
├── metrics.rs     # atomic counters
└── event.rs       # event definition
```

Each module has **one responsibility**.

---

## 🧪 Supported behavior

* Thousands of concurrent TCP clients
* Bounded queues to prevent OOM
* Parallel event processing
* Correct shared state updates
* Lock-free performance metrics
* Graceful shutdown

---

## 📊 Metrics tracked

* total events received
* total events processed
* total events dropped

Metrics are **atomic** and non-blocking.

---

## 🛑 Backpressure behavior

When the system is overloaded:

* Incoming queue fills up
* New events are rejected
* Clients are told to retry

This keeps the server **alive and predictable**.

---

## 🧱 STEP-BY-STEP BUILD PLAN (IMPORTANT)

Build this **incrementally**. Do NOT code everything at once.

---

### ✅ STEP 1 — Minimal TCP server

Goal:

* Accept TCP connections
* Read lines
* Print incoming data

What you learn:

* Async I/O basics
* Tokio runtime

---

### ✅ STEP 2 — Parse JSON events

Goal:

* Convert input into `Event` struct
* Reject invalid input

What you learn:

* Ownership of parsed data
* Error handling

---

### ✅ STEP 3 — Async bounded channel (backpressure)

Goal:

* Push events into a bounded Tokio channel
* Reject when full

What you learn:

* Backpressure
* Why unbounded queues are dangerous

---

### ✅ STEP 4 — Worker thread pool

Goal:

* Spawn OS threads
* Pull events from a channel
* Process in parallel

What you learn:

* True parallelism
* Difference between async and threads

---

### ✅ STEP 5 — Bridge async → threads

Goal:

* Forward events from Tokio channel to Crossbeam channel

What you learn:

* Async vs blocking boundaries
* System architecture thinking

---

### ✅ STEP 6 — Shared state (Arc + Lock)

Goal:

* Maintain per-user balances
* Update safely from many threads

What you learn:

* Data races
* Lock contention
* RwLock vs Mutex

---

### ✅ STEP 7 — Lock-free metrics

Goal:

* Track counters using atomics

What you learn:

* Why locks are bad for hot paths
* Atomic memory ordering

---

### ✅ STEP 8 — Client responses (ACK / NACK)

Goal:

* Respond immediately to clients
* Tell them to retry if busy

What you learn:

* Real-world protocol design

---

### ✅ STEP 9 — Graceful shutdown

Goal:

* Stop accepting new connections
* Drain queues
* Join threads

What you learn:

* Production-grade shutdown

---

## 🎯 What you will understand after finishing

| Concept      | You will *feel* it |
| ------------ | ------------------ |
| Async        | Non-blocking I/O   |
| Threads      | Parallelism        |
| Channels     | Flow control       |
| Locks        | Contention         |
| Atomics      | Performance        |
| Backpressure | Stability          |

---

## 🧠 Final takeaway

If you can build this **end-to-end**:

* Async Rust will make sense
* Concurrency bugs won’t scare you
* Reading systems code becomes easy

This is **not a toy project**.

It’s a mental upgrade.

---

## 📌 Next possible extensions

* DashMap instead of RwLock
* Work-stealing scheduler
* Persistence (disk / database)
* HTTP instead of TCP
* Benchmarking & flamegraphs

---

🔥 Build it slowly. Measure everything. Break it on purpose.
