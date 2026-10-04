//! Minimal HTTP/1.1 mock server for provider tests (no external crates).

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub head: String,
    pub body: String,
}

impl CapturedRequest {
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("request body is JSON")
    }
}

pub struct MockServer {
    addr: std::net::SocketAddr,
    captured: Arc<Mutex<Vec<CapturedRequest>>>,
}

impl MockServer {
    /// Serve each `(status, content_type, body)` in order, one per connection.
    pub async fn start(responses: Vec<(u16, &'static str, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let cap = Arc::clone(&captured);
        tokio::spawn(async move {
            for (status, ctype, body) in responses {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let req = read_request(&mut sock).await;
                cap.lock().await.push(req);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        Self { addr, captured }
    }

    /// Accepts a request, sends headers, then never sends a body.
    pub async fn start_stalling() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let _ = read_request(&mut sock).await;
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n").await;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        Self { addr, captured: Arc::new(Mutex::new(Vec::new())) }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub async fn requests(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }
}

async fn read_request(sock: &mut TcpStream) -> CapturedRequest {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = sock.read(&mut tmp).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
            let len = head
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            while buf.len() < pos + 4 + len {
                let n = sock.read(&mut tmp).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            let body = String::from_utf8_lossy(&buf[pos + 4..]).to_string();
            return CapturedRequest { head, body };
        }
    }
    CapturedRequest { head: String::new(), body: String::new() }
}
