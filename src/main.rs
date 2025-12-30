use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("Accepted connection from {}", addr);

        // Spawn a new async task to handle the connection concurrently
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                eprintln!("Error handling connection: {:?}", e);
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> tokio::io::Result<()> {
    let mut reader = BufReader::new(&mut stream);

    let mut content_length = 0;

    // Read HTTP headers
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            // Connection closed
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

    // Read the request body
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;

    // Echo the body back
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n",
        body.len()
    );

    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;

    Ok(())
}
