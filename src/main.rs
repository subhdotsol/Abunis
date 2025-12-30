use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();

    for stream in listener.incoming() {
        let stream = stream.unwrap();
        println!("Accepted connection");

        handle_connection(stream);

        println!("Connection established");
    }
}

fn handle_connection(mut stream: TcpStream) {
    let mut reader = BufReader::new(&stream);

    let mut content_length = 0;
    // let mut content_length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(len) = line.strip_prefix("Content-Length: ") {
            // content_length = len.parse().unwrap();
            content_length = len.parse::<usize>().unwrap();
        }
    }

    // read request body
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).unwrap();

    // echo the body back to the client
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Content-Length: {}\r\n\
            \r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
    stream.write_all(&body).unwrap();
    stream.flush().unwrap();
}
