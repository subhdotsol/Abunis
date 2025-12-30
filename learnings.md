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

