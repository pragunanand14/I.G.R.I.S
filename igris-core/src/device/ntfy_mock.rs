//! A small stand-in for an ntfy server (https://docs.ntfy.sh), for tests:
//! `POST /<topic>` publishes (bodies over 4096 bytes are refused, like ntfy
//! would turn them into attachments), `GET /<t1,t2,…>/json?since=…` streams
//! newline-delimited JSON events (`open`, `message`, `keepalive`), with
//! cached messages replayed according to `since` (`all`, a duration like
//! `15m`, a Unix time, or a message id).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

#[derive(Clone)]
struct Msg {
    id: String,
    time: i64,
    topic: String,
    body: String,
}

#[derive(Default)]
struct State {
    messages: Vec<Msg>,
    subs: Vec<(HashSet<String>, mpsc::UnboundedSender<String>)>,
    published: usize,
    serial: u64,
}

#[derive(Clone)]
pub struct MockNtfy {
    pub url: String,
    state: Arc<Mutex<State>>,
}

impl MockNtfy {
    pub async fn start() -> MockNtfy {
        Self::start_on("127.0.0.1:0").await
    }

    pub async fn start_on(addr: &str) -> MockNtfy {
        let listener = TcpListener::bind(addr).await.expect("bind mock ntfy");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let st = state.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let st = st.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, st).await;
                });
            }
        });
        MockNtfy { url, state }
    }

    /// Messages published so far.
    pub fn published(&self) -> usize {
        self.state.lock().unwrap().published
    }

    /// Bodies published to topics, for checks that nothing readable leaks.
    pub fn bodies(&self) -> Vec<String> {
        self.state.lock().unwrap().messages.iter().map(|m| m.body.clone()).collect()
    }

    /// Drop every live subscription (as if the service restarted).
    pub fn drop_subscribers(&self) {
        self.state.lock().unwrap().subs.clear();
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

async fn handle(mut sock: TcpStream, st: Arc<Mutex<State>>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if buf.len() > 64 * 1024 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default().to_string();
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string());
    let len: usize =
        lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let valid_topic = |t: &str| !t.is_empty() && t.len() <= 64 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if method == "POST" || method == "PUT" {
        let topic = path.trim_start_matches('/').to_string();
        if !valid_topic(&topic) {
            return respond(&mut sock, 400, "{\"error\":\"invalid topic\"}").await;
        }
        if body.len() > 4096 {
            return respond(&mut sock, 413, "{\"error\":\"too large\"}").await;
        }
        let msg = {
            let mut s = st.lock().unwrap();
            s.serial += 1;
            let m = Msg { id: format!("m{:012}", s.serial), time: now(), topic: topic.clone(), body: String::from_utf8_lossy(&body).to_string() };
            s.messages.push(m.clone());
            s.published += 1;
            let line = event_line(&m);
            s.subs.retain(|(topics, tx)| !topics.contains(&topic) || tx.send(line.clone()).is_ok());
            m
        };
        return respond(&mut sock, 200, &json!({"id": msg.id, "time": msg.time, "event": "message", "topic": msg.topic, "message": msg.body}).to_string())
            .await;
    }

    // GET /t1,t2/json?since=…
    let Some(topics) = path.trim_start_matches('/').strip_suffix("/json") else {
        return respond(&mut sock, 404, "{}").await;
    };
    let topics: HashSet<String> = topics.split(',').map(str::to_string).collect();
    if topics.iter().any(|t| !valid_topic(t)) {
        return respond(&mut sock, 400, "{}").await;
    }
    let since = query.split('&').find_map(|kv| kv.strip_prefix("since=")).unwrap_or("").to_string();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let backlog: Vec<String> = {
        let mut s = st.lock().unwrap();
        let msgs: Vec<&Msg> = s.messages.iter().filter(|m| topics.contains(&m.topic)).collect();
        let from: Vec<&Msg> = if since == "all" {
            msgs
        } else if let Some(d) = parse_duration(&since) {
            msgs.into_iter().filter(|m| m.time >= now() - d).collect()
        } else if let Ok(ts) = since.parse::<i64>() {
            msgs.into_iter().filter(|m| m.time >= ts).collect()
        } else if !since.is_empty() {
            match msgs.iter().position(|m| m.id == since) {
                Some(i) => msgs[i + 1..].to_vec(),
                None => msgs,
            }
        } else {
            Vec::new()
        };
        let lines = from.into_iter().map(event_line).collect();
        s.subs.push((topics.clone(), tx));
        lines
    };
    sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson; charset=utf-8\r\nConnection: close\r\n\r\n").await?;
    let topic_list = topics.iter().cloned().collect::<Vec<_>>().join(",");
    sock.write_all(format!("{}\n", json!({"id": "open", "time": now(), "event": "open", "topic": topic_list})).as_bytes()).await?;
    for l in backlog {
        sock.write_all(l.as_bytes()).await?;
    }
    sock.flush().await?;
    let mut keepalive = tokio::time::interval(Duration::from_secs(30));
    keepalive.tick().await;
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(l) => sock.write_all(l.as_bytes()).await?,
                None => return Ok(()),
            },
            _ = keepalive.tick() => sock.write_all(format!("{}\n", json!({"id": "ka", "time": now(), "event": "keepalive", "topic": topic_list})).as_bytes()).await?,
        }
        sock.flush().await?;
    }
}

fn event_line(m: &Msg) -> String {
    format!("{}\n", json!({"id": m.id, "time": m.time, "event": "message", "topic": m.topic, "message": m.body}))
}

fn parse_duration(s: &str) -> Option<i64> {
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = n.parse().ok()?;
    Some(match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => return None,
    })
}

async fn respond(sock: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    sock.write_all(
        format!("HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes(),
    )
    .await?;
    sock.shutdown().await
}
