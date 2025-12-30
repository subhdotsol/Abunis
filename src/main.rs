use std::{collections::HashMap, sync::Arc, thread};

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

struct Job {
    event: Event,
}

type State = Arc<Mutex<HashMap<String, i64>>>;

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    let state = Arc::new(Mutex::new(HashMap::new()));

    let (tx, rx) = mpsc::channel::<Job>(100);

    // ---- CORRECTED: wrap receiver in Arc<Mutex<>> ONCE ----
    let rx = Arc::new(Mutex::new(rx));

    let worker_count = 4;
    for i in 0..worker_count {
        let rx = rx.clone();
        let worker_state = state.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                loop {
                    // ---- LOCK receiver ----
                    let job_option = {
                        let mut rx_locked = rx.lock().await;
                        rx_locked.recv().await
                    };

                    if let Some(job) = job_option {
                        // simulate slow processing
                        sleep(Duration::from_millis(500)).await;

                        let mut state = worker_state.lock().await;
                        let balance = state.entry(job.event.user).or_insert(0);
                        *balance += job.event.amount;

                        println!(
                            "[Worker {}] Processed job, new balance = {}",
                            i,
                            balance
                        );
                    } else {
                        break; // channel closed
                    }
                }
            });
        });
    }

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("Accepted connection from {}", addr);

        let tx = tx.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, tx).await {
                eprintln!("Error handling connection: {:?}", e);
            }
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    tx: mpsc::Sender<Job>,
) -> tokio::io::Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut content_length = 0;

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

        // HTTP headers are case-insensitive (RFC 7230)
        let line_lower = line.to_lowercase();
        if let Some(len) = line_lower.strip_prefix("content-length: ") {
            content_length = len.trim().parse::<usize>().unwrap_or(0);
        }
    }

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;

    let response_body = match serde_json::from_slice::<Event>(&body) {
        Ok(event) => match tx.try_send(Job { event }) {
            Ok(_) => r#"{"status":"accepted"}"#.to_string(),
            Err(_) => r#"{"status":"rejected","reason":"server_busy"}"#.to_string(),
        },
        Err(_) => r#"{"status":"error","reason":"invalid_json"}"#.to_string(),
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        response_body.len(),
        response_body
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}
