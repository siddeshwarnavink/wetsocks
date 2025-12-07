mod constants;
pub mod http;
pub mod service;
pub mod ws;

use std::collections::HashMap;
use std::process::exit;
use std::sync::Arc;

use lazy_static::lazy_static;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::service::{User};

lazy_static! {
    static ref USERS: Mutex<HashMap<String, User>> = Mutex::new(HashMap::new());
}

#[tokio::main]
async fn main() {
    let addr = "0.0.0.0:3333";
    let listener = TcpListener::bind(addr).await.unwrap_or_else(|_| {
        eprintln!("Error: Failed to listen to {}", addr);
        exit(1);
    });

    println!("Listening to http://{}/", addr);

    loop {
        if let Ok((stream, _)) = listener.accept().await {
            let shared_stream = Arc::new(Mutex::new(stream));

            tokio::spawn(async move {
                if let Err(err) =
                    service::request_handler(shared_stream.clone()).await
                {
                    let msg = err.to_string();
                    eprintln!("[error] {msg}");

                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\n\
                         Content-Type: text/plain\r\n\
                         Content-Length: {}\r\n\r\n{}",
                        msg.len(),
                        msg
                    );

                    let mut stream = shared_stream.lock().await;
                    let _ = stream.write_all(response.as_bytes()).await;
                }
            });
        }
    }
}
