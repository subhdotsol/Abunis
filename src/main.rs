use std::{collections::HashMap, sync::Arc};

use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc}, 
    time::{Duration, sleep},
};

#[derive(Debug, Deserialize)]
struct Event {
    user: String,
    action: String,
    amount: i64,
}

// ---- CHANGE: job sent through async queue ----
struct Job {
    event: Event,
}

// Shared state (unchanged)
type State = Arc<Mutex<HashMap<String, i64>>>;

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    let state = Arc::new(Mutex::new(HashMap::new()));

    // ---- CHANGE: bounded async channel (BACKPRESSURE POINT) ----
    let (tx, mut rx) = mpsc::channel::<Job>(100);

    // ---- CHANGE: dedicated worker task ----
    // This task owns ALL slow processing + state mutation
    let worker_state = state.clone();
    tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            // ---- INTENTIONALLY SLOW PROCESSING ----
            sleep(Duration::from_millis(500)).await;

            let mut state = worker_state.lock().await;
            let balance = state.entry(job.event.user).or_insert(0);
            *balance += job.event.amount;

            println!("Processed job, new balance = {}", balance);
        }
    });

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("Accepted connection from {}", addr);

        let tx = tx.clone(); // <-- CHANGE: pass sender, not state

        // Spawn a new async task per connection
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, tx).await {
                eprintln!("Error handling connection: {:?}", e);
            }
        });
    }
}

// ---- CHANGE: handler no longer touches shared state ----
// It ONLY parses input and tries to enqueue work
async fn handle_connection(
    mut stream: TcpStream,
    tx: mpsc::Sender<Job>,
) -> tokio::io::Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut content_length = 0;

    // ---- Read HTTP headers ----
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(());
        }

        let line = line.trim();
        if line.is_empty() {
            break;
        }

        if let Some(len) = line.strip_prefix("Content-Length: ") {
            content_length = len.parse::<usize>().unwrap_or(0);
        }
    }

    // ---- Read request body ----
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;

    // ---- Parse JSON + enqueue ----
    let response_body = match serde_json::from_slice::<Event>(&body) {
        Ok(event) => {
            // ---- CHANGE: non-blocking enqueue ----
            match tx.try_send(Job { event }) {
                Ok(_) => {
                    // Accepted for async processing
                    r#"{"status":"accepted"}"#.to_string()
                }
                Err(_) => {
                    // BACKPRESSURE: queue is full
                    r#"{"status":"rejected","reason":"server_busy"}"#.to_string()
                }
            }
        }
        Err(_) => {
            r#"{"status":"error","reason":"invalid_json"}"#.to_string()
        }
    };

    // ---- Send HTTP response ----
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n{}",
        response_body.len(),
        response_body
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}
