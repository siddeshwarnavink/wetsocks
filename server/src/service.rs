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

use crate::USERS;
use crate::constants::*;
use crate::http::header::{self, HttpHeader, HttpVerb};
use crate::ws::frame;

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
    SendMessage {
        recipient: String,
        payload: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        group_id: Option<String>,
    },

    #[serde(rename = "relay_message")]
    RelayMessage {
        sender: String,
        payload: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        group_id: Option<String>,
    },

    #[serde(rename = "first")]
    First { public_key: String, name: String },

    #[serde(rename = "new_user")]
    NewUser { user: User },

    #[serde(rename = "user_left")]
    UserLeft { user_id: String },
}

async fn client_request_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
    mut buf: BytesMut,
    ws_id: String,
) -> io::Result<()> {
    let mut user_public_key: Option<String> = None;

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
            if let Some(ref public_key) = user_public_key {
                user_leave(public_key).await;
            }
            break Ok(());
        }

        if let Ok(req_json) = frame::get_text(&buf[..len]) {
            if req_json.len() < 1 {
                println!("[error] invalid request from {ws_id}");
                yield_now().await;
                continue;
            }

            let req = match serde_json::from_str(&req_json) {
                Ok(j) => j,
                Err(_) => {
                    println!("[error] invalid JSON from {ws_id}");
                    println!("[error] json = {}", req_json);
                    yield_now().await;
                    continue;
                }
            };

            let shared_stream = shared_stream.clone();

            match req {
                Payload::First { public_key, name } => {
                    user_public_key = Some(public_key.clone());
                    let pk = public_key.clone();
                    tokio::spawn(async move {
                        user_join(
                            pk.as_str(),
                            name.as_str(),
                            public_key.as_str(),
                            shared_stream.clone(),
                        )
                        .await;
                        dispatch_all_keys(
                            public_key.as_str(),
                            shared_stream.clone(),
                        )
                        .await;
                    });
                }
                Payload::SendMessage {
                    recipient,
                    payload,
                    group_id,
                } => {
                    if let Some(ref sender_pk) = user_public_key {
                        let sender = sender_pk.clone();
                        tokio::spawn(async move {
                            relay_message(
                                sender.as_str(),
                                recipient.as_str(),
                                payload.as_str(),
                                group_id,
                            )
                            .await;
                        });
                    }
                }
                _ => {}
            }
        }

        yield_now().await;
    }
}

async fn dispatch_all_keys(
    public_key: &str,
    shared_stream: Arc<Mutex<TcpStream>>,
) {
    let users = USERS.lock().await;
    let user = match users.get(public_key) {
        Some(u) => u,
        None => return,
    };

    let user_data = Payload::NewUser { user: user.clone() };
    let user_json = match serde_json::to_string(&user_data) {
        Ok(j) => j,
        Err(_) => return,
    };

    let mut buf = BytesMut::with_capacity(4096);
    buf.reserve(1024);

    let mut stream = shared_stream.lock().await;

    for (other_public_key, user) in users.iter() {
        if other_public_key == public_key {
            continue;
        }

        let mut other_user_stream = user.shared_stream.lock().await;

        buf.clear();
        let len = frame::set_text(&mut buf, &user_json);
        let _ = other_user_stream.write_all(&buf[..len]).await;

        drop(other_user_stream);

        let other_user_data = Payload::NewUser { user: user.clone() };
        let other_user_json = match serde_json::to_string(&other_user_data) {
            Ok(j) => j,
            Err(_) => continue,
        };

        buf.clear();
        let len = frame::set_text(&mut buf, &other_user_json);
        let _ = stream.write_all(&buf[..len]).await;
    }
}

async fn relay_message(
    sender: &str,
    recipient: &str,
    payload: &str,
    group_id: Option<String>,
) {
    let users = USERS.lock().await;

    if let Some(user) = users.get(recipient) {
        let mut stream = user.shared_stream.lock().await;

        let mut buf = BytesMut::with_capacity(4096);
        buf.reserve(1024);

        let msg = Payload::RelayMessage {
            sender: sender.to_string(),
            payload: payload.to_string(),
            group_id,
        };

        if let Ok(json) = serde_json::to_string(&msg) {
            let len = frame::set_text(&mut buf, &json);
            let _ = stream.write_all(&buf[..len]).await;
        }
    }
}

async fn static_resource_handler(
    shared_stream: Arc<Mutex<TcpStream>>,
    filename: &str,
) -> io::Result<()> {
    let mut stream = shared_stream.lock().await;

    let path = format!("./static/{}", filename);

    match fs::metadata(&path).await {
        Ok(metadata) if metadata.is_file() => {
            let mime_type = match path.rsplit('.').next() {
                Some("html") => "text/html",
                Some("css") => "text/css",
                Some("js") => "application/javascript",
                Some("wasm") => "application/wasm",
                _ => {
                    let body = "404 Not Found - Unsupported file type";
                    let header = format!(
                        "HTTP/1.1 404 Not Found\r\n\
                         Content-Type: text/plain; charset=utf-8\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\
                         \r\n",
                        body.len()
                    );
                    stream.write_all(header.as_bytes()).await?;
                    stream.write_all(body.as_bytes()).await?;
                    stream.flush().await?;
                    return Ok(());
                }
            };

            let body = fs::read(&path).await?;
            let header = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: {}; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n",
                mime_type,
                body.len()
            );

            stream.write_all(header.as_bytes()).await?;
            stream.write_all(&body).await?;
            stream.flush().await?;
        }
        _ => {
            let body = "404 Not Found";
            let header = format!(
                "HTTP/1.1 404 Not Found\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n",
                body.len()
            );

            stream.write_all(header.as_bytes()).await?;
            stream.write_all(body.as_bytes()).await?;
            stream.flush().await?;
        }
    }

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
            static_resource_handler(shared_stream.clone(), "index.html").await
        }
        (HttpVerb::Get, "/ws") => {
            ws_handler(shared_stream.clone(), http_header, buf).await
        }
        _ => {
            static_resource_handler(
                shared_stream.clone(),
                http_header.path.as_str(),
            )
            .await
        }
    }
}

async fn user_join(
    public_key: &str,
    name: &str,
    public_key_copy: &str,
    shared_stream: Arc<Mutex<TcpStream>>,
) {
    let mut users = USERS.lock().await;
    let new_user = User {
        id: public_key.into(),
        name: name.into(),
        shared_stream: shared_stream.clone(),
        public_key: Some(public_key_copy.into()),
    };

    users.insert(public_key.into(), new_user);
}

async fn user_leave(public_key: &str) {
    let mut users = USERS.lock().await;
    users.remove(public_key);

    let mut buf = BytesMut::with_capacity(4096);
    buf.reserve(1024);

    let msg = Payload::UserLeft {
        user_id: public_key.to_string(),
    };

    for (_, user) in users.iter() {
        let mut stream = user.shared_stream.lock().await;

        if let Ok(json) = serde_json::to_string(&msg) {
            buf.clear();
            let len = frame::set_text(&mut buf, &json);
            let _ = stream.write_all(&buf[..len]).await;
        }
    }
}
