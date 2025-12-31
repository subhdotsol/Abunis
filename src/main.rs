use std::{sync::Arc, thread};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc, broadcast},
    signal,
    time::{Duration, sleep, timeout},
};

// ===== Request types =====
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Request {
    Metrics { r#type: String },
    Event(Event),
}

#[derive(Debug, Deserialize)]
struct Event {
    user: String,
    action: String,
    amount: i64,
}

struct Job {
    event: Event,
}

type State = Arc<DashMap<String, i64>>;

// ===== Lock-free metrics =====
struct Metrics {
    received: AtomicU64,
    processed: AtomicU64,
    dropped: AtomicU64,
    total_lock_wait_ns: AtomicU64,
}

impl Metrics {
    fn new() -> Self {
        Self {
            received: AtomicU64::new(0),
            processed: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            total_lock_wait_ns: AtomicU64::new(0),
        }
    }
}

#[derive(Serialize)]
struct MetricsResponse {
    received: u64,
    processed: u64,
    dropped: u64,
    pending: u64,
}

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");
    println!("===== STAGE 10: Graceful Shutdown =====");
    println!("Press Ctrl+C to trigger graceful shutdown\n");

    let state: State = Arc::new(DashMap::new());
    let metrics = Arc::new(Metrics::new());

    let (tx, rx) = mpsc::channel::<Job>(1000);
    let rx = Arc::new(Mutex::new(rx));

    // ===== STAGE 10: Shutdown signal =====
    let (shutdown_tx, _) = broadcast::channel::<()>(1);
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    let worker_count = 16;
    println!("Starting {} workers", worker_count);

    // ===== STAGE 10: Track worker threads =====
    let mut worker_handles = vec![];

    for i in 0..worker_count {
        let rx = rx.clone();
        let worker_state = state.clone();
        let worker_metrics = metrics.clone();
        let worker_shutdown = shutdown_flag.clone();

        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                loop {
                    // Check shutdown flag
                    if worker_shutdown.load(Ordering::Relaxed) {
                        // Try to drain remaining jobs
                        let job_option = {
                            let mut rx_locked = rx.lock().await;
                            // Non-blocking check
                            match tokio::time::timeout(
                                Duration::from_millis(100),
                                rx_locked.recv()
                            ).await {
                                Ok(job) => job,
                                Err(_) => None, // Timeout = no more jobs
                            }
                        };
                        
                        if job_option.is_none() {
                            println!("[Worker {:2}] Shutdown complete, no more jobs", i);
                            break;
                        }
                        
                        // Process remaining job
                        if let Some(job) = job_option {
                            process_job(&worker_state, &worker_metrics, &job, i).await;
                        }
                    } else {
                        // Normal operation
                        let job_option = {
                            let mut rx_locked = rx.lock().await;
                            rx_locked.recv().await
                        };

                        if let Some(job) = job_option {
                            process_job(&worker_state, &worker_metrics, &job, i).await;
                        } else {
                            // Channel closed
                            println!("[Worker {:2}] Channel closed, exiting", i);
                            break;
                        }
                    }
                }
            });
        });
        
        worker_handles.push(handle);
    }

    // ===== STAGE 10: Metrics reporter with shutdown =====
    let reporter_metrics = metrics.clone();
    let reporter_shutdown = shutdown_flag.clone();
    let reporter_handle = thread::spawn(move || {
        let mut last_processed = 0u64;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            
            if reporter_shutdown.load(Ordering::Relaxed) {
                println!("[Reporter] Shutting down");
                break;
            }
            
            let received = reporter_metrics.received.load(Ordering::Relaxed);
            let processed = reporter_metrics.processed.load(Ordering::Relaxed);
            let dropped = reporter_metrics.dropped.load(Ordering::Relaxed);
            let throughput = (processed - last_processed) as f64 / 2.0;
            
            if received > 0 {
                println!(
                    "\n[THROUGHPUT] {:.1} jobs/sec | Recv: {} | Proc: {} | Drop: {}\n",
                    throughput, received, processed, dropped
                );
            }
            last_processed = processed;
        }
    });

    // ===== STAGE 10: Main loop with graceful shutdown =====
    let mut shutdown_rx = shutdown_tx.subscribe();
    let accept_shutdown = shutdown_flag.clone();
    let final_metrics = metrics.clone();
    
    // Spawn Ctrl+C handler
    let shutdown_tx_clone = shutdown_tx.clone();
    let shutdown_flag_clone = shutdown_flag.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        println!("\n");
        println!("🛑 ========================================");
        println!("🛑 SHUTDOWN SIGNAL RECEIVED (Ctrl+C)");
        println!("🛑 ========================================");
        println!("🛑 Step 1: Stop accepting new connections...");
        
        shutdown_flag_clone.store(true, Ordering::Relaxed);
        let _ = shutdown_tx_clone.send(());
    });

    // Accept loop
    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        if accept_shutdown.load(Ordering::Relaxed) {
                            println!("[Accept] Rejecting connection from {} (shutting down)", addr);
                            continue;
                        }
                        
                        let tx = tx.clone();
                        let conn_metrics = metrics.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, tx, conn_metrics).await {
                                eprintln!("Error handling connection: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Accept error: {:?}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                println!("🛑 Step 2: Accept loop stopped");
                break;
            }
        }
    }

    // ===== STAGE 10: Graceful shutdown sequence =====
    println!("🛑 Step 3: Closing job channel...");
    drop(tx);  // Close sender → workers will drain and exit

    println!("🛑 Step 4: Waiting for workers to drain queue (max 10s)...");
    
    // Wait for workers with timeout
    let shutdown_timeout = Duration::from_secs(10);
    let start = Instant::now();
    
    for (i, handle) in worker_handles.into_iter().enumerate() {
        let remaining = shutdown_timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            println!("⚠️  Timeout! Force exiting (some workers may not have finished)");
            break;
        }
        
        // Wait for thread with timeout
        let wait_result = std::thread::spawn(move || handle.join());
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        if start.elapsed() < shutdown_timeout {
            println!("✅ Worker {} finished", i);
        }
    }

    println!("🛑 Step 5: Waiting for reporter...");
    let _ = reporter_handle.join();

    // Print final metrics
    let received = final_metrics.received.load(Ordering::Relaxed);
    let processed = final_metrics.processed.load(Ordering::Relaxed);
    let dropped = final_metrics.dropped.load(Ordering::Relaxed);
    
    println!();
    println!("📊 ========================================");
    println!("📊 FINAL METRICS");
    println!("📊 ========================================");
    println!("📊 Received:  {}", received);
    println!("📊 Processed: {}", processed);
    println!("📊 Dropped:   {}", dropped);
    println!("📊 Pending:   {} (lost on shutdown)", received.saturating_sub(processed).saturating_sub(dropped));
    println!();
    println!("✅ Graceful shutdown complete!");
    
    Ok(())
}

// ===== Job processing helper =====
async fn process_job(state: &State, metrics: &Metrics, job: &Job, worker_id: usize) {
    sleep(Duration::from_millis(10)).await;

    let wait_start = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(1));
    
    *state.entry(job.event.user.clone()).or_insert(0) += job.event.amount;
    
    let _wait_duration = wait_start.elapsed();
    let _new_balance = *state.get(&job.event.user).unwrap();

    metrics.total_lock_wait_ns.fetch_add(
        _wait_duration.as_nanos() as u64, 
        Ordering::Relaxed
    );
    let jobs = metrics.processed.fetch_add(1, Ordering::Relaxed) + 1;

    if jobs % 100 == 0 {
        let received = metrics.received.load(Ordering::Relaxed);
        let dropped = metrics.dropped.load(Ordering::Relaxed);
        println!(
            "[METRICS] Received: {} | Processed: {} | Dropped: {}",
            received, jobs, dropped
        );
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    tx: mpsc::Sender<Job>,
    metrics: Arc<Metrics>,
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

        let line_lower = line.to_lowercase();
        if let Some(len) = line_lower.strip_prefix("content-length: ") {
            content_length = len.trim().parse::<usize>().unwrap_or(0);
        }
    }

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;

    metrics.received.fetch_add(1, Ordering::Relaxed);

    let response_body = match serde_json::from_slice::<Request>(&body) {
        Ok(Request::Metrics { r#type }) if r#type == "metrics" => {
            let response = MetricsResponse {
                received: metrics.received.load(Ordering::Relaxed),
                processed: metrics.processed.load(Ordering::Relaxed),
                dropped: metrics.dropped.load(Ordering::Relaxed),
                pending: metrics.received.load(Ordering::Relaxed) 
                    - metrics.processed.load(Ordering::Relaxed) 
                    - metrics.dropped.load(Ordering::Relaxed),
            };
            serde_json::to_string(&response).unwrap()
        }
        Ok(Request::Event(event)) => {
            match tx.try_send(Job { event }) {
                Ok(_) => r#"{"status":"accepted"}"#.to_string(),
                Err(_) => {
                    metrics.dropped.fetch_add(1, Ordering::Relaxed);
                    r#"{"status":"rejected","reason":"server_busy"}"#.to_string()
                }
            }
        }
        Ok(Request::Metrics { .. }) => {
            r#"{"status":"error","reason":"invalid_type"}"#.to_string()
        }
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
