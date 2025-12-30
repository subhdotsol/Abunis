use std::{collections::HashMap, sync::Arc, thread};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

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

// ===== STAGE 7: Lock contention metrics =====
struct Metrics {
    total_lock_wait_ns: AtomicU64,
    total_lock_hold_ns: AtomicU64,
    jobs_processed: AtomicU64,
}

impl Metrics {
    fn new() -> Self {
        Self {
            total_lock_wait_ns: AtomicU64::new(0),
            total_lock_hold_ns: AtomicU64::new(0),
            jobs_processed: AtomicU64::new(0),
        }
    }
}

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    let state = Arc::new(Mutex::new(HashMap::new()));
    let metrics = Arc::new(Metrics::new());

    let (tx, rx) = mpsc::channel::<Job>(100);
    let rx = Arc::new(Mutex::new(rx));

    // ===== STAGE 7: Increased to 16 workers to show contention =====
    let worker_count = 16;
    println!("Starting {} workers (Stage 7: Lock Contention Demo)", worker_count);

    for i in 0..worker_count {
        let rx = rx.clone();
        let worker_state = state.clone();
        let worker_metrics = metrics.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                loop {
                    let job_option = {
                        let mut rx_locked = rx.lock().await;
                        rx_locked.recv().await
                    };

                    if let Some(job) = job_option {
                        // Simulate slow processing BEFORE lock
                        sleep(Duration::from_millis(100)).await;

                        // ===== MEASURE LOCK WAIT TIME =====
                        let wait_start = Instant::now();
                        let mut state = worker_state.lock().await;
                        let wait_duration = wait_start.elapsed();
                        
                        // ===== MEASURE LOCK HOLD TIME =====
                        let hold_start = Instant::now();
                        
                        // Simulate work while holding lock (this is the problem!)
                        // In real systems this might be DB writes, serialization, etc.
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        
                        let balance = state.entry(job.event.user.clone()).or_insert(0);
                        *balance += job.event.amount;
                        let new_balance = *balance;
                        
                        let hold_duration = hold_start.elapsed();
                        drop(state); // Explicitly release lock

                        // Record metrics
                        worker_metrics.total_lock_wait_ns.fetch_add(
                            wait_duration.as_nanos() as u64, 
                            Ordering::Relaxed
                        );
                        worker_metrics.total_lock_hold_ns.fetch_add(
                            hold_duration.as_nanos() as u64, 
                            Ordering::Relaxed
                        );
                        let jobs = worker_metrics.jobs_processed.fetch_add(1, Ordering::Relaxed) + 1;

                        // Print every 50 jobs
                        if jobs % 50 == 0 {
                            let avg_wait = worker_metrics.total_lock_wait_ns.load(Ordering::Relaxed) / jobs;
                            let avg_hold = worker_metrics.total_lock_hold_ns.load(Ordering::Relaxed) / jobs;
                            println!(
                                "[METRICS] Jobs: {} | Avg lock wait: {:.2}ms | Avg lock hold: {:.2}ms",
                                jobs,
                                avg_wait as f64 / 1_000_000.0,
                                avg_hold as f64 / 1_000_000.0
                            );
                        }

                        println!(
                            "[Worker {:2}] user={}, balance={}, wait={:.2}ms, hold={:.2}ms",
                            i,
                            job.event.user,
                            new_balance,
                            wait_duration.as_secs_f64() * 1000.0,
                            hold_duration.as_secs_f64() * 1000.0
                        );
                    } else {
                        break;
                    }
                }
            });
        });
    }

    // Metrics reporter thread
    let reporter_metrics = metrics.clone();
    thread::spawn(move || {
        let mut last_jobs = 0u64;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let current_jobs = reporter_metrics.jobs_processed.load(Ordering::Relaxed);
            let throughput = (current_jobs - last_jobs) as f64 / 2.0;
            if current_jobs > 0 {
                println!(
                    "\n[THROUGHPUT] {:.1} jobs/sec | Total: {} jobs\n",
                    throughput, current_jobs
                );
            }
            last_jobs = current_jobs;
        }
    });

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
