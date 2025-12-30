use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() {
    let mut tasks = vec![];

    for i in 1..=20 {
        tasks.push(tokio::spawn(async move {
            let mut stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
            let body = format!("{{\"hello\":\"world{}\"}}", i);
            let request = format!(
                "POST / HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );

            // Send request
            stream.write_all(request.as_bytes()).await.unwrap();

            // Read response
            let mut response = vec![];
            stream.read_to_end(&mut response).await.unwrap();

            println!("Client {} got: {}", i, String::from_utf8_lossy(&response));
        }));
    }

    // Wait for all clients
    for t in tasks {
        t.await.unwrap();
    }
}
