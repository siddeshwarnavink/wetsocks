use std::io;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::BytesMut;
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
use crate::{MESSAGES, USERS};

pub enum MessageKind {
    ServerMessage,
    UserMessage,
}

pub struct Message {
    pub kind: MessageKind,
    pub text: String,
}

pub struct User {
    pub name: String,
    pub shared_stream: Arc<Mutex<TcpStream>>,
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

        if let Ok(req) = frame::get_text(&buf[..len]) {
            if req.len() < 4 {
                println!("[error] invalid request from {user_id}");
                yield_now().await;
                continue;
            }

            let cmd = &req[..3];
            let payload = &req[4..];

            match cmd {
                CMD_MESSAGE => {
                    post_user_message(user_id.as_str(), payload).await
                }
                CMD_RENAME => user_rename(user_id.as_str(), payload).await,
                _ => println!("[error] invalid command {cmd} from {user_id}"),
            }
        }

        yield_now().await;
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

    let body = fs::read_to_string(&path).await?;
    let response = format!(
        "HTTP/1.1 200 Ok\r\n\
         Content-Type: {}; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        mime_type,
        body.len(),
        body
    );

    stream.write_all(response.as_bytes()).await?;
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
        (HttpVerb::Get, "/ws") => {
            ws_handler(shared_stream.clone(), http_header, buf).await
        }
        _ => not_found_handler(shared_stream.clone()).await,
    }
}

async fn user_join(id: &str, shared_stream: Arc<Mutex<TcpStream>>, name: &str) {
    let mut users = USERS.lock().await;
    let new_user = User {
        name: name.into(),
        shared_stream: shared_stream.clone(),
    };

    users.insert(id.into(), new_user);
    println!("[info] New user {id} joined.");
}

async fn user_leave(id: &str) {
    let mut users = USERS.lock().await;

    users.remove(id);
    println!("[info] User {id} left.");
}

async fn user_rename(id: &str, name: &str) {
    let mut users = USERS.lock().await;
    if let Some(user) = users.get_mut(id) {
        user.name = name.into();
        println!("[info] User {id} changed name to {name}.");
    }

    drop(users);
    post_server_message(&format!("{} joined the chat.", name)).await;
}

async fn post_user_message(id: &str, text: &str) {
    let users = USERS.lock().await;
    let mut messages = MESSAGES.lock().await;
    if let Some(user) = users.get(id) {
        let new_msg = Message {
            kind: MessageKind::ServerMessage,
            text: format!("{}: {}", user.name, text),
        };

        messages.push(new_msg);
        println!("[info] User {id} posted new message.");

        drop(users);
        drop(messages);

        dispatch_messages().await;
    }
}

async fn post_server_message(text: &str) {
    let mut messages = MESSAGES.lock().await;
    let new_msg = Message {
        kind: MessageKind::ServerMessage,
        text: text.into(),
    };

    messages.push(new_msg);
    println!("[info] New message \"{text}\".");

    drop(messages);

    dispatch_messages().await;
}

async fn dispatch_messages() {
    let mut buf = BytesMut::with_capacity(4096);
    buf.reserve(1024);

    let mut messages = MESSAGES.lock().await;
    if messages.len() < 1 {
        return;
    }

    let users = USERS.lock().await;

    while let Some(message) = messages.pop() {
        for (_, user) in users.iter() {
            let mut stream = user.shared_stream.lock().await;
            let response = format!("{}", message.text);

            buf.clear();
            let len = frame::set_text(&mut buf, &response);

            let _ = stream.write_all(&buf[..len]).await;
        }
    }
}
