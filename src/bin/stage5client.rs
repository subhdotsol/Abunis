use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SERVER_ADDR: &str = "127.0.0.1:8080";
const CONCURRENT_CLIENTS: usize = 50;
const REQUESTS_PER_CLIENT: usize = 20;

#[tokio::main]
async fn main() {
    let accepted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();

    for client_id in 0..CONCURRENT_CLIENTS {
        let accepted = accepted.clone();
        let rejected = rejected.clone();
        let errors = errors.clone();

        let handle = tokio::spawn(async move {
            for _ in 0..REQUESTS_PER_CLIENT {
                let body = format!(
                    r#"{{"user":"user{}","action":"add","amount":1}}"#,
                    client_id
                );

                let request = format!(
                    "POST / HTTP/1.1\r\n\
                     Host: localhost\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {}\r\n\
                     \r\n{}",
                    body.len(),
                    body
                );

                match TcpStream::connect(SERVER_ADDR).await {
                    Ok(mut stream) => {
                        if stream.write_all(request.as_bytes()).await.is_err() {
                            errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }

                        let mut response = vec![0; 512];
                        match stream.read(&mut response).await {
                            Ok(n) => {
                                let text = String::from_utf8_lossy(&response[..n]);
                                if text.contains(r#""status":"accepted""#) {
                                    accepted.fetch_add(1, Ordering::Relaxed);
                                } else if text.contains(r#""server_busy""#) {
                                    rejected.fetch_add(1, Ordering::Relaxed);
                                } else {
                                    errors.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Err(_) => {
                                errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }

    println!("=== Stress Test Results ===");
    println!("Accepted : {}", accepted.load(Ordering::Relaxed));
    println!("Rejected : {}", rejected.load(Ordering::Relaxed));
    println!("Errors   : {}", errors.load(Ordering::Relaxed));
}
