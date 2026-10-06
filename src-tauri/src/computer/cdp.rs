//! A minimal Chrome DevTools Protocol client.
//!
//! Only what IGRIS's browser control needs: a WebSocket connection to one page
//! target on `127.0.0.1`, request/response by id, events ignored. The browser
//! is one IGRIS starts itself with its own profile (see `browser.rs`), so the
//! endpoint is local and never exposed beyond this machine.

use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Largest message accepted from the browser (DOM snapshots are bounded well below this).
const MAX_MESSAGE: usize = 32 * 1024 * 1024;

pub struct Cdp {
    stream: BufReader<TcpStream>,
    next_id: u64,
    pub timeout: Duration,
}

fn bad(e: impl std::fmt::Display) -> String {
    format!("Browser connection failed: {e}")
}

/// GET a small JSON document from the DevTools HTTP endpoint. (The browser
/// keeps the connection open, so the body is read by its Content-Length.)
pub async fn http_json(port: u16, path: &str) -> Result<Value, String> {
    let s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(("127.0.0.1", port))).await.map_err(bad)?.map_err(bad)?;
    let mut s = BufReader::new(s);
    s.get_mut().write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes()).await.map_err(bad)?;
    let read = async {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(s.read_u8().await.map_err(bad)?);
            if head.len() > 16 * 1024 {
                return Err("DevTools response headers too large.".to_string());
            }
        }
        let head = String::from_utf8_lossy(&head).to_lowercase();
        let len: usize =
            head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).ok_or("DevTools response without a length.")?;
        if len > MAX_MESSAGE {
            return Err("DevTools response too large.".into());
        }
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).await.map_err(bad)?;
        serde_json::from_slice(&body).map_err(bad)
    };
    tokio::time::timeout(Duration::from_secs(5), read).await.map_err(bad)?
}

impl Cdp {
    /// Connect to a target's WebSocket URL (`ws://127.0.0.1:PORT/devtools/page/ID`).
    pub async fn connect(ws_url: &str) -> Result<Self, String> {
        let rest = ws_url.strip_prefix("ws://").ok_or("Only local ws:// DevTools endpoints are supported.")?;
        let (host, path) = rest.split_once('/').ok_or("Malformed DevTools URL.")?;
        if !(host.starts_with("127.0.0.1:") || host.starts_with("localhost:")) {
            return Err("Refusing a non-local DevTools endpoint.".into());
        }
        let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(host)).await.map_err(bad)?.map_err(bad)?;
        tcp.set_nodelay(true).ok();
        let key = base64::engine::general_purpose::STANDARD.encode(uuid::Uuid::new_v4().as_bytes());
        let mut stream = BufReader::new(tcp);
        let req = format!(
            "GET /{path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.get_mut().write_all(req.as_bytes()).await.map_err(bad)?;
        // Read the handshake response headers.
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let b = tokio::time::timeout(Duration::from_secs(5), stream.read_u8()).await.map_err(bad)?.map_err(bad)?;
            head.push(b);
            if head.len() > 16 * 1024 {
                return Err("DevTools handshake too large.".into());
            }
        }
        let status = String::from_utf8_lossy(&head);
        if !status.starts_with("HTTP/1.1 101") {
            return Err(format!("DevTools refused the connection: {}", status.lines().next().unwrap_or_default()));
        }
        Ok(Self { stream, next_id: 1, timeout: Duration::from_secs(20) })
    }

    async fn send_frame(&mut self, payload: &[u8]) -> Result<(), String> {
        let mut frame = vec![0x81u8]; // FIN + text
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        // Client frames are masked (RFC 6455 §5.3).
        let mask: [u8; 4] = uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap_or([1, 2, 3, 4]);
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.get_mut().write_all(&frame).await.map_err(bad)
    }

    /// Read one complete text message (handling continuation, ping and close).
    async fn read_message(&mut self) -> Result<String, String> {
        let mut message = Vec::new();
        loop {
            let b0 = self.stream.read_u8().await.map_err(bad)?;
            let b1 = self.stream.read_u8().await.map_err(bad)?;
            let fin = b0 & 0x80 != 0;
            let opcode = b0 & 0x0F;
            let mut len = (b1 & 0x7F) as u64;
            if len == 126 {
                len = self.stream.read_u16().await.map_err(bad)? as u64;
            } else if len == 127 {
                len = self.stream.read_u64().await.map_err(bad)?;
            }
            if len as usize > MAX_MESSAGE || message.len() + len as usize > MAX_MESSAGE {
                return Err("The browser sent a message that was too large.".into());
            }
            let mask = if b1 & 0x80 != 0 {
                let mut m = [0u8; 4];
                self.stream.read_exact(&mut m).await.map_err(bad)?;
                Some(m)
            } else {
                None
            };
            let mut data = vec![0u8; len as usize];
            self.stream.read_exact(&mut data).await.map_err(bad)?;
            if let Some(m) = mask {
                for (i, b) in data.iter_mut().enumerate() {
                    *b ^= m[i % 4];
                }
            }
            match opcode {
                0x8 => return Err("The browser closed the connection.".into()),
                0x9 => {
                    // Ping → pong.
                    let mut pong = vec![0x8A, 0x80 | data.len().min(125) as u8, 0, 0, 0, 0];
                    pong.extend_from_slice(&data[..data.len().min(125)]);
                    self.stream.get_mut().write_all(&pong).await.map_err(bad)?;
                    continue;
                }
                0xA => continue,
                _ => message.extend_from_slice(&data),
            }
            if fin {
                return String::from_utf8(message).map_err(bad);
            }
        }
    }

    /// Send a command and wait for its result (events in between are skipped).
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_frame(json!({ "id": id, "method": method, "params": params }).to_string().as_bytes()).await?;
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            let msg = tokio::time::timeout_at(deadline, self.read_message()).await.map_err(|_| format!("The browser didn't answer {method} in time."))??;
            let v: Value = serde_json::from_str(&msg).map_err(bad)?;
            if v["id"].as_u64() == Some(id) {
                if let Some(e) = v.get("error") {
                    return Err(format!("The browser rejected {method}: {}", e["message"].as_str().unwrap_or("unknown error")));
                }
                return Ok(v["result"].clone());
            }
        }
    }

    /// Evaluate JavaScript in the page and return its (JSON) value.
    pub async fn eval(&mut self, expression: &str) -> Result<Value, String> {
        let r = self.call("Runtime.evaluate", json!({ "expression": expression, "returnByValue": true, "awaitPromise": true })).await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(format!("Page script error: {}", ex["exception"]["description"].as_str().or(ex["text"].as_str()).unwrap_or("unknown")));
        }
        Ok(r["result"]["value"].clone())
    }
}
