use serde_json::json;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let server_url = "http://127.0.0.1:8080"; // your Stage 6 server
    let total_requests = 100;                 // reduce for testing
    let concurrency = 10;

    let sem = Arc::new(Semaphore::new(concurrency));
    let mut handles = vec![];

    for i in 0..total_requests {
        let sem_clone = sem.clone();
        let url = server_url.to_string();

        let handle = tokio::spawn(async move {
            let _permit = sem_clone.acquire_owned().await.unwrap();

            let client = reqwest::Client::new();
            let event = json!({
                "user": format!("user{}", i % 10),
                "action": "sell",
                "amount": 10
            });

            let body = event.to_string();

            let res = client.post(&url)
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await;

            match res {
                Ok(resp) => {
                    let text = resp.text().await.unwrap_or_default();
                    println!("Request {} got response: {}", i, text);
                }
                Err(err) => {
                    println!("Request {} failed: {}", i, err);
                }
            }

            sleep(Duration::from_millis(1)).await;
        });

        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    println!("Stress test completed!");
}
