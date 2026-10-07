//! The relay over real sockets: authentication, routing rules, presence,
//! offline queue and pairing limits.

use std::time::Duration;

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use igris_relay::proto::{self, ClientFrame, SendStatus, ServerFrame, B64};
use igris_relay::{Config, Relay};
use rand_core::OsRng;
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn start(cfg: Config) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", l.local_addr().unwrap());
    tokio::spawn(Relay::new(cfg).serve(l));
    url
}

struct Dev {
    key: SigningKey,
    id: String,
}

fn dev() -> Dev {
    let key = SigningKey::generate(&mut OsRng);
    let id = proto::device_id_for(key.verifying_key().as_bytes());
    Dev { key, id }
}

async fn send(ws: &mut Ws, f: &ClientFrame) {
    ws.send(Message::text(serde_json::to_string(f).unwrap())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> Option<ServerFrame> {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => return Some(serde_json::from_str(&t).unwrap()),
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => return None,
            Ok(Some(Ok(_))) => continue,
            Err(_) => panic!("no frame within 5s"),
        }
    }
}

/// Connect and answer the challenge with `signer` claiming to be `claimed`.
async fn connect_as(url: &str, signer: &SigningKey, claimed: &str) -> (Ws, Option<ServerFrame>) {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let Some(ServerFrame::Challenge { nonce, protocol }) = recv(&mut ws).await else { panic!("no challenge") };
    assert_eq!(protocol, proto::PROTOCOL);
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let sig = signer.sign(&proto::auth_bytes(&nonce, claimed, ts));
    let key = B64.encode(signer.verifying_key().as_bytes());
    send(&mut ws, &ClientFrame::Auth { device: claimed.into(), key, ts, sig: B64.encode(sig.to_bytes()) }).await;
    let answer = recv(&mut ws).await;
    (ws, answer)
}

async fn connect(url: &str, d: &Dev) -> Ws {
    let (ws, answer) = connect_as(url, &d.key, &d.id).await;
    assert!(matches!(answer, Some(ServerFrame::Ready { .. })), "{answer:?}");
    ws
}

#[tokio::test]
async fn devices_must_prove_their_key() {
    let url = start(Config::default()).await;
    let a = dev();
    let b = dev();
    // Signed by another key than the one presented / claimed.
    let (_, answer) = connect_as(&url, &b.key, &a.id).await;
    assert!(matches!(answer, Some(ServerFrame::Error { .. })), "{answer:?}");
    // Valid.
    connect(&url, &a).await;
    // No auth at all: a normal frame first is refused.
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    recv(&mut ws).await;
    send(&mut ws, &ClientFrame::Peers { ids: vec![] }).await;
    assert!(matches!(recv(&mut ws).await, Some(ServerFrame::Error { .. }) | None));
}

#[tokio::test]
async fn a_restricted_relay_only_accepts_listed_devices() {
    let a = dev();
    let b = dev();
    let url = start(Config { allow: [a.id.clone()].into(), ..Default::default() }).await;
    connect(&url, &a).await;
    let (_, answer) = connect_as(&url, &b.key, &b.id).await;
    assert!(matches!(answer, Some(ServerFrame::Error { .. })));
}

#[tokio::test]
async fn routing_needs_both_devices_to_list_each_other() {
    let url = start(Config::default()).await;
    let (a, b, c) = (dev(), dev(), dev());
    let mut wa = connect(&url, &a).await;
    let mut wb = connect(&url, &b).await;
    let mut wc = connect(&url, &c).await;
    send(&mut wa, &ClientFrame::Peers { ids: vec![b.id.clone()] }).await;
    // Only A lists B so far: no presence, and sending is refused.
    send(&mut wa, &ClientFrame::Send { id: "m1".into(), to: b.id.clone(), env: serde_json::json!({"x":1}) }).await;
    assert!(matches!(recv(&mut wa).await, Some(ServerFrame::Sent { status: SendStatus::Rejected, .. })));
    send(&mut wb, &ClientFrame::Peers { ids: vec![a.id.clone()] }).await;
    assert_eq!(recv(&mut wb).await, Some(ServerFrame::Presence { device: a.id.clone(), online: true }));
    assert_eq!(recv(&mut wa).await, Some(ServerFrame::Presence { device: b.id.clone(), online: true }));
    send(&mut wa, &ClientFrame::Send { id: "m2".into(), to: b.id.clone(), env: serde_json::json!({"x":2}) }).await;
    assert!(matches!(recv(&mut wa).await, Some(ServerFrame::Sent { status: SendStatus::Delivered, .. })));
    assert_eq!(recv(&mut wb).await, Some(ServerFrame::Deliver { from: a.id.clone(), env: serde_json::json!({"x":2}) }));
    // C (a stranger who lists A) can't reach A and never learns A's presence.
    send(&mut wc, &ClientFrame::Peers { ids: vec![a.id.clone()] }).await;
    send(&mut wc, &ClientFrame::Send { id: "m3".into(), to: a.id.clone(), env: serde_json::json!({}) }).await;
    assert!(matches!(recv(&mut wc).await, Some(ServerFrame::Sent { status: SendStatus::Rejected, .. })));
    // B leaves: A is told it went offline.
    drop(wb);
    assert_eq!(recv(&mut wa).await, Some(ServerFrame::Presence { device: b.id.clone(), online: false }));
}

#[tokio::test]
async fn offline_devices_get_a_bounded_queue() {
    let url = start(Config { queue_per_device: 2, ..Default::default() }).await;
    let (a, b) = (dev(), dev());
    let mut wa = connect(&url, &a).await;
    let mut wb = connect(&url, &b).await;
    send(&mut wa, &ClientFrame::Peers { ids: vec![b.id.clone()] }).await;
    send(&mut wb, &ClientFrame::Peers { ids: vec![a.id.clone()] }).await;
    recv(&mut wa).await; // presence
    drop(wb);
    assert!(matches!(recv(&mut wa).await, Some(ServerFrame::Presence { online: false, .. })));
    for (i, want) in [SendStatus::Queued, SendStatus::Queued, SendStatus::Rejected].into_iter().enumerate() {
        send(&mut wa, &ClientFrame::Send { id: format!("q{i}"), to: b.id.clone(), env: serde_json::json!({"n": i}) }).await;
        match recv(&mut wa).await {
            Some(ServerFrame::Sent { status, .. }) => assert_eq!(status, want),
            other => panic!("{other:?}"),
        }
    }
    // B comes back and gets the two it held, in order.
    let mut wb = connect(&url, &b).await;
    send(&mut wb, &ClientFrame::Peers { ids: vec![a.id.clone()] }).await;
    let mut got = Vec::new();
    while got.len() < 2 {
        if let Some(ServerFrame::Deliver { env, .. }) = recv(&mut wb).await {
            got.push(env["n"].as_i64().unwrap());
        }
    }
    assert_eq!(got, vec![0, 1]);
}

#[tokio::test]
async fn pairing_rendezvous_is_limited() {
    let url = start(Config { pair_attempts: 2, ..Default::default() }).await;
    let (inviter, joiner) = (dev(), dev());
    let mut wi = connect(&url, &inviter).await;
    let mut wj = connect(&url, &joiner).await;
    send(&mut wi, &ClientFrame::PairOpen { pid: "0123456789".into() }).await;
    assert!(matches!(recv(&mut wi).await, Some(ServerFrame::PairStatus { ok: true, .. })));
    // The inviter can only answer a device that sent a request.
    send(&mut wi, &ClientFrame::PairReply { pid: "0123456789".into(), to: joiner.id.clone(), blob: "x".into() }).await;
    assert!(matches!(recv(&mut wi).await, Some(ServerFrame::PairStatus { ok: false, .. })));
    for _ in 0..2 {
        send(&mut wj, &ClientFrame::PairSend { pid: "0123456789".into(), blob: "guess".into() }).await;
        assert!(matches!(recv(&mut wj).await, Some(ServerFrame::PairStatus { ok: true, .. })));
        assert!(matches!(recv(&mut wi).await, Some(ServerFrame::PairMsg { .. })));
    }
    // Attempts used up.
    send(&mut wj, &ClientFrame::PairSend { pid: "0123456789".into(), blob: "guess".into() }).await;
    assert!(matches!(recv(&mut wj).await, Some(ServerFrame::PairStatus { ok: false, .. })));
    // Unknown code.
    send(&mut wj, &ClientFrame::PairSend { pid: "aaaaaaaaaa".into(), blob: "x".into() }).await;
    assert!(matches!(recv(&mut wj).await, Some(ServerFrame::PairStatus { ok: false, .. })));
}

#[tokio::test]
async fn floods_and_garbage_close_the_connection() {
    let url = start(Config { rate_per_10s: 5, ..Default::default() }).await;
    let a = dev();
    let mut wa = connect(&url, &a).await;
    for _ in 0..10 {
        let _ = wa.send(Message::text(serde_json::to_string(&ClientFrame::Ping).unwrap())).await;
    }
    let mut closed = false;
    for _ in 0..12 {
        match recv(&mut wa).await {
            Some(ServerFrame::Error { .. }) | None => {
                closed = true;
                break;
            }
            _ => {}
        }
    }
    assert!(closed);
    let mut wa = connect(&url, &a).await;
    wa.send(Message::text("{\"t\":\"exec\",\"cmd\":\"ls\"}")).await.unwrap();
    assert!(matches!(recv(&mut wa).await, Some(ServerFrame::Error { .. }) | None));
}
