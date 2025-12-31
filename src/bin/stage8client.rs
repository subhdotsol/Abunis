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

    println!("=== Stage 8: DashMap Sharding Test ===");
    println!("Total requests: {}", total_requests);
    println!("Concurrency: {}", concurrency);
    println!();
    println!("Expected: WAY fewer rejections than Stage 7!");
    println!();

    let sem = Arc::new(Semaphore::new(concurrency));
    let mut handles = vec![];

    let accepted = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let total_latency_ms = Arc::new(AtomicU64::new(0));

    let test_start = Instant::now();

    for i in 0..total_requests {
        let sem_clone = sem.clone();
        let url = server_url.to_string();
        let accepted = accepted.clone();
        let rejected = rejected.clone();
        let errors = errors.clone();
        let total_latency = total_latency_ms.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem_clone.acquire_owned().await.unwrap();

            let client = reqwest::Client::new();
            let event = json!({
                "user": format!("user{}", i % 100),  // 100 different users (spread across shards)
                "action": "deposit",
                "amount": 10
            });

            let request_start = Instant::now();
            
            let res = client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(event.to_string())
                .send()
                .await;

            let latency = request_start.elapsed().as_millis() as u64;
            total_latency.fetch_add(latency, Ordering::Relaxed);

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
    let avg_latency = total_latency_ms.load(Ordering::Relaxed) as f64 / total_requests as f64;
    let throughput = total_requests as f64 / test_duration.as_secs_f64();

    println!();
    println!("=== Results ===");
    println!("Duration:     {:.2}s", test_duration.as_secs_f64());
    println!("Throughput:   {:.1} requests/sec", throughput);
    println!("Avg latency:  {:.1}ms", avg_latency);
    println!();
    println!("Accepted:     {}", total_accepted);
    println!("Rejected:     {} (backpressure)", total_rejected);
    println!("Errors:       {}", total_errors);
    println!();
    
    // Compare with Stage 7
    println!("=== Comparison with Stage 7 ===");
    println!("Stage 7: Accepted ~138, Rejected ~4862 (97% rejected)");
    println!("Stage 8: Accepted {}, Rejected {} ({:.1}% rejected)", 
        total_accepted, 
        total_rejected,
        (total_rejected as f64 / total_requests as f64) * 100.0
    );
    
    if total_rejected < 1000 {
        println!();
        println!("🎉 SUCCESS! DashMap sharding dramatically reduced rejections!");
        println!("   Workers can now update different users in parallel.");
    }
}
