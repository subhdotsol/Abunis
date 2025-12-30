use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::main]
async fn main() {
    let mut handles = Vec::new();

    for i in 1..=10 {
        handles.push(tokio::spawn(async move {
            // Connect to the server
            let mut stream = TcpStream::connect("127.0.0.1:8080")
                .await
                .expect("Failed to connect");

            // Prepare JSON body
            let body = format!(r#"{{"user":"user{}","action":"buy","amount":10}}"#, i);

            // Prepare HTTP request
            let request = format!(
                "POST / HTTP/1.1\r\n\
                 Host: localhost\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 \r\n{}",
                body.len(),
                body
            );

            // Send request
            stream.write_all(request.as_bytes()).await.unwrap();

            // Read response
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();

            let response_text = String::from_utf8_lossy(&response);
            println!("Response for user{}:\n{}\n", i, response_text);
        }));
    }

    // Wait for all tasks to finish
    for handle in handles {
        handle.await.unwrap();
    }
}
