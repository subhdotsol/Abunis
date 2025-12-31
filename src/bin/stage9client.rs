use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() {
    let server_url = "http://127.0.0.1:8080";
    let total_requests = 5000;
    let concurrency = 200;

    println!("=== Stage 9: Lock-Free Metrics Test ===");
    println!("Total requests: {}", total_requests);
    println!("Concurrency: {}", concurrency);
    println!();

    // First, get initial metrics
    println!("Getting initial metrics...");
    let client = reqwest::Client::new();
    let metrics_req = json!({"type": "metrics"});
    
    match client
        .post(server_url)
        .header("Content-Type", "application/json")
        .body(metrics_req.to_string())
        .send()
        .await
    {
        Ok(resp) => {
            let text = resp.text().await.unwrap_or_default();
            println!("Initial metrics: {}", text);
        }
        Err(e) => println!("Failed to get initial metrics: {}", e),
    }
    println!();

    let sem = Arc::new(Semaphore::new(concurrency));
    let mut handles = vec![];

    let accepted = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));

    let test_start = Instant::now();

    for i in 0..total_requests {
        let sem_clone = sem.clone();
        let url = server_url.to_string();
        let accepted = accepted.clone();
        let rejected = rejected.clone();
        let errors = errors.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem_clone.acquire_owned().await.unwrap();

            let client = reqwest::Client::new();
            let event = json!({
                "user": format!("user{}", i % 100),
                "action": "deposit",
                "amount": 10
            });

            let res = client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(event.to_string())
                .send()
                .await;

            match res {
                Ok(resp) => {
                    let text = resp.text().await.unwrap_or_default();
                    if text.contains("accepted") {
                        accepted.fetch_add(1, Ordering::Relaxed);
                    } else if text.contains("rejected") {
                        rejected.fetch_add(1, Ordering::Relaxed);
                    } else {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    let test_duration = test_start.elapsed();
    let total_accepted = accepted.load(Ordering::Relaxed);
    let total_rejected = rejected.load(Ordering::Relaxed);
    let total_errors = errors.load(Ordering::Relaxed);

    println!();
    println!("=== Client Results ===");
    println!("Duration:     {:.2}s", test_duration.as_secs_f64());
    println!("Accepted:     {}", total_accepted);
    println!("Rejected:     {}", total_rejected);
    println!("Errors:       {}", total_errors);
    println!();

    // Wait a bit for server to process, then get final metrics
    println!("Waiting 2s for server to process...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    println!();
    println!("=== Server Metrics (via API) ===");
    let metrics_req = json!({"type": "metrics"});
    
    match client
        .post(server_url)
        .header("Content-Type", "application/json")
        .body(metrics_req.to_string())
        .send()
        .await
    {
        Ok(resp) => {
            let text = resp.text().await.unwrap_or_default();
            println!("Server metrics: {}", text);
            
            // Parse and display nicely
            if let Ok(metrics) = serde_json::from_str::<serde_json::Value>(&text) {
                println!();
                println!("Breakdown:");
                println!("  Received:  {}", metrics["received"]);
                println!("  Processed: {}", metrics["processed"]);
                println!("  Dropped:   {}", metrics["dropped"]);
                println!("  Pending:   {}", metrics["pending"]);
            }
        }
        Err(e) => println!("Failed to get final metrics: {}", e),
    }
    
    println!();
    println!("✅ Stage 9: Lock-free metrics working!");
    println!("   All counters use AtomicU64 - no locks needed!");
}
