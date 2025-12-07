use std::io;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::yield_now;
use tokio::time::timeout;

use crate::constants::*;
use crate::http::header::{self, HttpHeader, HttpVerb};
use crate::ws::frame;
use crate::{USERS};

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub sender: String,
    pub payload: String,
}

fn default_stream() -> Arc<Mutex<TcpStream>> {
    panic!("shared_stream should never be deserialized");
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub id: String,
    pub name: String,
    pub public_key: Option<String>,

    #[serde(skip)]
    #[serde(default = "default_stream")]
    pub shared_stream: Arc<Mutex<TcpStream>>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Payload {
    #[serde(rename = "send_message")]
    SendMessage { recipient: String, payload: String },

    #[serde(rename = "relay_message")]
    RelayMessage { sender: String, payload: String },

    #[serde(rename = "first")]
    First { public_key: String, name: String },

    #[serde(rename = "new_user")]
    NewUser { user: User },
}

async fn client_request_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
    mut buf: BytesMut,
    user_id: String,
) -> io::Result<()> {
    loop {
        let mut stream = shared_stream.lock().await;

        if let Err(_) =
            timeout(Duration::from_millis(10), stream.readable()).await
        {
            drop(stream);
            yield_now().await;
            continue;
        }

        buf.clear();

        let len = stream.read_buf(&mut buf).await?;
        drop(stream);

        if len < 1 {
            user_leave(&user_id).await;
            break Ok(());
        }

        if let Ok(req_json) = frame::get_text(&buf[..len]) {
            if req_json.len() < 1 {
                println!("[error] invalid request from {user_id}");
                yield_now().await;
                continue;
            }

            let req = match serde_json::from_str(&req_json) {
                Ok(j) => j,
                Err(_) => {
                    println!("[error] invalid JSON from {user_id}");
                    println!("[error] json = {}", req_json);
                    yield_now().await;
                    continue;
                }
            };

            let shared_stream = shared_stream.clone();
            let user_id = user_id.clone();

            tokio::spawn(async move {
                match req {
                    Payload::First { public_key, name } => {
                        user_first_setup(
                            user_id.as_str(),
                            name.as_str(),
                            public_key.as_str(),
                        )
                        .await;
                        dispatch_all_keys(
                            user_id.as_str(),
                            shared_stream.clone(),
                        )
                        .await;
                    }
                    Payload::SendMessage { recipient, payload } => {
                        relay_message(user_id.as_str(), recipient.as_str(), payload.as_str()).await;
                    }
                    _ => {}
                }
            });
        }

        yield_now().await;
    }
}

async fn dispatch_all_keys(
    user_id: &str,
    shared_stream: Arc<Mutex<TcpStream>>,
) {
    let users = USERS.lock().await;
    let user = match users.get(user_id) {
        Some(u) => u,
        None => return,
    };

    let user_data = Payload::NewUser {
        user: user.clone(),
    };
    let user_json = match serde_json::to_string(&user_data) {
        Ok(j) => j,
        Err(_) => return,
    };


    let mut buf = BytesMut::with_capacity(4096);
    buf.reserve(1024);

    let mut stream = shared_stream.lock().await;

    for (_, user) in users.iter() {
        if user.id == user_id {
            continue;
        }

        let mut other_user_stream = user.shared_stream.lock().await;

        buf.clear();
        let len = frame::set_text(&mut buf, &user_json);
        let _ = other_user_stream.write_all(&buf[..len]).await;

        drop(other_user_stream);

        let other_user_data = Payload::NewUser {
            user: user.clone(),
        };
        let other_user_json = match serde_json::to_string(&other_user_data) {
            Ok(j) => j,
            Err(_) => continue,
        };

        buf.clear();
        let len = frame::set_text(&mut buf, &other_user_json);
        let _ = stream.write_all(&buf[..len]).await;
    }
}

async fn relay_message(sender: &str, recipient: &str, payload: &str) {
    let users = USERS.lock().await;

    if let Some(user) = users.get(recipient) {
        let mut stream = user.shared_stream.lock().await;

        let mut buf = BytesMut::with_capacity(4096);
        buf.reserve(1024);

        let msg = Payload::RelayMessage {
            sender: sender.to_string(),
            payload: payload.to_string(),
        };

        if let Ok(json) = serde_json::to_string(&msg) {
            let len = frame::set_text(&mut buf, &json);
            let _ = stream.write_all(&buf[..len]).await;
        }
    }
}

async fn not_found_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
) -> io::Result<()> {
    let mut stream = shared_stream.lock().await;

    let body = "404 Not Found";
    let response = format!(
        "HTTP/1.1 404 Not Found\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        body.len(),
        body
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}

async fn static_resource_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
    filename: &str,
    mime_type: &str,
) -> io::Result<()> {
    let mut stream = shared_stream.lock().await;

    let path = format!("./static/{filename}");

    let body = fs::read(&path).await?;
    let header = format!(
        "HTTP/1.1 200 Ok\r\n\
         Content-Type: {}; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        mime_type,
        body.len(),
    );

    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;

    Ok(())
}

async fn ws_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
    http_header: HttpHeader,
    buf: BytesMut,
) -> io::Result<()> {
    let mut stream = shared_stream.lock().await;

    if let Some(val) = http_header.table.get("Upgrade") {
        if val != "websocket" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                ERR_WS_CONNECTION,
            ));
        }
    }

    if let Some(val) = http_header.table.get("Sec-WebSocket-Version") {
        if val != "13" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                ERR_WS_VERSION,
            ));
        }
    }

    if let Some(key) = http_header.table.get("Sec-WebSocket-Key") {
        let combined = format!("{}{}", key, WS_GUID);

        let mut hasher = Sha1::new();
        hasher.update(combined.as_bytes());
        let hashed = hasher.finalize();
        let user_id = B64.encode(hashed);

        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            user_id
        );
        stream.write_all(response.as_bytes()).await?;

        drop(response);
        drop(http_header);
        drop(stream);

        user_join(&user_id, shared_stream.clone(), DEFAULT_NAME).await;

        client_request_handler(
            shared_stream.clone(),
            buf.into(),
            user_id.into(),
        )
        .await?;
    }

    Ok(())
}

pub async fn request_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
) -> io::Result<()> {
    let mut stream = shared_stream.lock().await;

    let mut buf = BytesMut::with_capacity(4096);
    buf.reserve(1024);

    let len = stream.read_buf(&mut buf).await?;
    let s = str::from_utf8(&buf[..len])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let http_header = header::parse(s).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "Invalid HTTP header.")
    })?;

    drop(stream);

    match (http_header.verb.clone(), http_header.path.as_str()) {
        (HttpVerb::Get, "/") => {
            static_resource_handler(
                shared_stream.clone(),
                "index.html",
                "text/html",
            )
            .await
        }
        (HttpVerb::Get, "/script.js") => {
            static_resource_handler(
                shared_stream.clone(),
                "script.js",
                "text/javascript",
            )
            .await
        }
        (HttpVerb::Get, "/main.css") => {
            static_resource_handler(
                shared_stream.clone(),
                "main.css",
                "text/css",
            )
            .await
        }
        (HttpVerb::Get, "/crypto_wasm.js") => {
            static_resource_handler(
                shared_stream.clone(),
                "crypto-wasm/crypto_wasm.js",
                "text/javascript",
            )
            .await
        }
        (HttpVerb::Get, "/crypto_wasm_bg.wasm") => {
            static_resource_handler(
                shared_stream.clone(),
                "crypto-wasm/crypto_wasm_bg.wasm",
                "application/wasm",
            )
            .await
        }
        (HttpVerb::Get, "/ws") => {
            ws_handler(shared_stream.clone(), http_header, buf).await
        }
        _ => not_found_handler(shared_stream.clone()).await,
    }
}

async fn user_join(id: &str, shared_stream: Arc<Mutex<TcpStream>>, name: &str) {
    let mut users = USERS.lock().await;
    let new_user = User {
        id: id.into(),
        name: name.into(),
        shared_stream: shared_stream.clone(),
        public_key: None
    };

    users.insert(id.into(), new_user);
    println!("[info] New user {id} joined.");
}

async fn user_leave(id: &str) {
    let mut users = USERS.lock().await;

    users.remove(id);
    println!("[info] User {id} left.");
}

async fn user_first_setup(id: &str, name: &str, public_key: &str) {
    let mut users = USERS.lock().await;
    if let Some(user) = users.get_mut(id) {
        user.name = name.into();
        user.public_key = Some(public_key.into());
    }
}
