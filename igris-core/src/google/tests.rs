//! The whole flow against a fake Google: sign-in (PKCE, state, loopback
//! redirect), refresh, Gmail and Calendar calls, revocation, meeting reminders.

use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::db::Database;

#[derive(Default)]
pub(crate) struct Seen {
    pub requests: Vec<(String, String, String, String)>,
    pub refresh_fails: bool,
    pub events: Vec<Value>,
    /// Messages listed besides m1.
    pub more_mail: Vec<String>,
}

pub(crate) type Shared = Arc<Mutex<Seen>>;

fn route(seen: &Shared, method: &str, target: &str, body: &str) -> (u16, Value) {
    let path = target.split('?').next().unwrap_or_default();
    let mut s = seen.lock().unwrap();
    match (method, path) {
        ("POST", "/token") if body.contains("grant_type=authorization_code") => {
            assert!(body.contains("code=the-code") && body.contains("code_verifier="), "{body}");
            (200, json!({"access_token": "at1", "refresh_token": "rt1", "expires_in": 3600, "scope": SCOPES.join(" ")}))
        }
        ("POST", "/token") if s.refresh_fails => (400, json!({"error": "invalid_grant"})),
        ("POST", "/token") => (200, json!({"access_token": "at2", "expires_in": 3600})),
        ("POST", "/revoke") => (200, json!({})),
        ("GET", "/gmail/profile") => (200, json!({"emailAddress": "ada@example.com"})),
        ("GET", "/gmail/messages") => {
            let mut ids: Vec<Value> = s.more_mail.iter().rev().map(|id| json!({"id": id})).collect();
            ids.push(json!({"id": "m1"}));
            (200, json!({ "messages": ids }))
        }
        ("GET", p) if p.starts_with("/gmail/messages/") && !p.ends_with("/modify") && !p.ends_with("/trash") => {
            let id = p.trim_start_matches("/gmail/messages/");
            let text = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("Can we meet at 5?\nRahul");
            (
                200,
                json!({"id": id, "threadId": "t1", "snippet": "Can we meet at 5?", "labelIds": ["UNREAD", "INBOX"], "payload": {
                    "headers": [{"name": "From", "value": "Rahul <rahul@example.com>"}, {"name": "Subject", "value": "Meeting"},
                                {"name": "Date", "value": "Mon, 6 Oct 2026 10:00:00 +0530"}, {"name": "Message-ID", "value": "<abc@mail>"}],
                    "mimeType": "multipart/alternative",
                    "parts": [{"mimeType": "text/plain", "body": {"data": text}}]
                }}),
            )
        }
        ("POST", "/gmail/drafts") => (200, json!({"id": "d1"})),
        ("POST", "/gmail/drafts/send") => (200, json!({"id": "sent1"})),
        ("POST", "/gmail/messages/m1/modify") | ("POST", "/gmail/messages/m1/trash") => (200, json!({})),
        ("GET", "/cal/calendars/primary/events") => (200, json!({"items": s.events.clone()})),
        ("POST", "/cal/calendars/primary/events") => {
            let v: Value = serde_json::from_str(body).unwrap();
            let e = json!({"id": "e9", "summary": v["summary"], "start": v["start"], "end": v["end"]});
            s.events.push(e.clone());
            (200, e)
        }
        ("DELETE", "/cal/calendars/primary/events/e9") => (204, Value::Null),
        _ => (404, json!({"error": {"message": "not found"}})),
    }
}

pub(crate) async fn fake_google(seen: Shared) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = l.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len = text[..h]
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if buf.len() >= h + 4 + len {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                let mut first = head.lines().next().unwrap_or_default().split_whitespace();
                let (method, target) = (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default().to_string());
                let auth = head.lines().find_map(|l| l.to_lowercase().starts_with("authorization:").then(|| l[14..].trim().to_string())).unwrap_or_default();
                seen.lock().unwrap().requests.push((method.clone(), target.clone(), body.to_string(), auth));
                let (status, v) = route(&seen, &method, &target, body);
                let out = if v.is_null() { String::new() } else { v.to_string() };
                let resp = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}", out.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    base
}

pub(crate) fn google(base: &str, db: Arc<Database>) -> Google {
    let ep = Endpoints {
        auth: format!("{base}/auth"),
        token: format!("{base}/token"),
        revoke: format!("{base}/revoke"),
        gmail: format!("{base}/gmail"),
        calendar: format!("{base}/cal"),
    };
    Google::with_endpoints(db, Arc::new(crate::device::Unprotected), Arc::new(|| Some(("cid".into(), "secret".into()))), ep)
}

/// A "browser" that follows the sign-in link: hits the redirect with a stray request first, then the code.
pub(crate) fn browser() -> Opener {
    Arc::new(|url: &str| {
        let u = reqwest::Url::parse(url).unwrap();
        let get = |k: &str| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned()).unwrap();
        assert_eq!(get("code_challenge_method"), "S256");
        assert!(get("scope").contains("gmail.modify") && get("scope").contains("calendar.events"));
        let (redirect, state) = (get("redirect_uri"), get("state"));
        assert!(redirect.starts_with("http://127.0.0.1:"));
        tokio::spawn(async move {
            let c = reqwest::Client::new();
            let _ = c.get(format!("{redirect}/favicon.ico")).send().await;
            let _ = c.get(format!("{redirect}/?state=wrong&code=evil")).send().await;
            let page = c.get(format!("{redirect}/?state={state}&code=the-code")).send().await.unwrap().text().await.unwrap();
            assert!(page.contains("IGRIS is connected"));
        });
        Ok(())
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_read_draft_send_and_disconnect() {
    let seen: Shared = Default::default();
    let base = fake_google(seen.clone()).await;
    let db = Arc::new(Database::open_in_memory().unwrap());
    let g = google(&base, db.clone());
    assert_eq!(g.status().unwrap(), Status { configured: true, connected: false, email: None });
    assert!(gmail::search(&g, "", 5).await.unwrap_err().to_string().contains("isn't connected"));

    assert_eq!(g.connect(&browser()).await.unwrap(), "ada@example.com");
    assert_eq!(g.status().unwrap().email.as_deref(), Some("ada@example.com"));
    let stored: Vec<u8> = db.conn().unwrap().query_row("SELECT refresh_sealed FROM google_account", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, b"rt1", "the refresh token is stored through the protector (pass-through in tests)");

    let found = gmail::search(&g, "is:unread", 10).await.unwrap();
    assert_eq!((found[0].from.as_str(), found[0].subject.as_str(), found[0].unread), ("Rahul <rahul@example.com>", "Meeting", true));
    let m = gmail::read(&g, "m1").await.unwrap();
    assert!(m.body.contains("Can we meet at 5?"));

    let d = gmail::draft(&g, gmail::NewDraft { to: "", cc: "", subject: "", body: "Yes, 5 works.", reply_to: Some("m1"), attachments: vec![] }).await.unwrap();
    assert_eq!((d.to.as_str(), d.subject.as_str()), ("Rahul <rahul@example.com>", "Re: Meeting"));
    assert_eq!(gmail::known_draft("d1").unwrap().body, "Yes, 5 works.");
    let raw_req = seen.lock().unwrap().requests.iter().find(|r| r.1 == "/gmail/drafts").unwrap().2.clone();
    let raw = serde_json::from_str::<Value>(&raw_req).unwrap()["message"]["raw"].as_str().unwrap().to_string();
    let mime = String::from_utf8(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw).unwrap()).unwrap();
    assert!(mime.contains("In-Reply-To: <abc@mail>") && mime.contains("Subject: Re: Meeting"));
    let pdf = gmail::FileAttachment { name: "Résumé.pdf".into(), mime: gmail::mime_for("Résumé.pdf").into(), bytes: b"%PDF-1.4 hello".to_vec() };
    let d = gmail::draft(&g, gmail::NewDraft { to: "hr@example.com", cc: "", subject: "CV", body: "Attached.", reply_to: None, attachments: vec![pdf] })
        .await
        .unwrap();
    assert_eq!(d.attachments, ["Résumé.pdf"]);
    let raw_req = seen.lock().unwrap().requests.iter().rev().find(|r| r.1 == "/gmail/drafts").unwrap().2.clone();
    let raw = serde_json::from_str::<Value>(&raw_req).unwrap()["message"]["raw"].as_str().unwrap().to_string();
    let mime = String::from_utf8(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw).unwrap()).unwrap();
    assert!(mime.contains("multipart/mixed") && mime.contains("Content-Type: application/pdf") && mime.contains("filename=\"=?UTF-8?B?"), "{mime}");
    assert!(mime.contains(&base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 hello")));
    let big = gmail::FileAttachment { name: "big.bin".into(), mime: "application/octet-stream".into(), bytes: vec![0; gmail::MAX_ATTACH_BYTES + 1] };
    assert!(gmail::draft(&g, gmail::NewDraft { to: "a@b.c", cc: "", subject: "x", body: "x", reply_to: None, attachments: vec![big] }).await.is_err());
    assert!(
        gmail::draft(&g, gmail::NewDraft { to: "a@b.c\r\nBcc: evil@x", cc: "", subject: "x", body: "", reply_to: None, attachments: vec![] }).await.is_err(),
        "no header injection"
    );
    assert_eq!(gmail::send_draft(&g, "d1").await.unwrap(), "sent1");
    assert_eq!(gmail::organise(&g, "m1", "archive").await.unwrap(), "Archived");

    // Every API call carried the access token; none carried the refresh token.
    assert!(seen.lock().unwrap().requests.iter().filter(|r| r.1.starts_with("/gmail")).all(|r| r.3 == "Bearer at1"));

    g.disconnect().await.unwrap();
    assert!(!g.is_connected());
    assert!(seen.lock().unwrap().requests.iter().any(|r| r.1 == "/revoke" && r.2.contains("token=rt1")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calendar_events_and_meeting_reminders() {
    let seen: Shared = Default::default();
    let base = fake_google(seen.clone()).await;
    let db = Arc::new(Database::open_in_memory().unwrap());
    let g = google(&base, db.clone());
    g.connect(&browser()).await.unwrap();

    let e = calendar::add(&g, calendar::NewEvent { title: "Dentist", start: "in 2 hours", minutes: 30, location: "", description: "", attendees: &[] })
        .await
        .unwrap();
    assert_eq!(e.title, "Dentist");
    let now = chrono::Utc::now();
    let listed = calendar::list(&g, now, now + chrono::Duration::days(1)).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(calendar::describe(&listed[0]).contains("Dentist"));

    // A reminder 10 minutes before, once; dropped when the event goes.
    assert_eq!(watcher::tick(&g, now).await.unwrap(), 1);
    assert_eq!(watcher::tick(&g, now).await.unwrap(), 0, "not twice");
    let pending = crate::productivity::reminders::active(&db.conn().unwrap()).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].title.starts_with("Dentist at "));
    calendar::delete(&g, "e9").await.unwrap();
    seen.lock().unwrap().events.clear();
    watcher::tick(&g, now).await.unwrap();
    assert!(crate::productivity::reminders::active(&db.conn().unwrap()).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_sign_in_disconnects_and_says_so() {
    let seen: Shared = Default::default();
    let base = fake_google(seen.clone()).await;
    let g = google(&base, Arc::new(Database::open_in_memory().unwrap()));
    g.connect(&browser()).await.unwrap();
    *g.token.lock().await = None; // the access token expired
    seen.lock().unwrap().refresh_fails = true;
    let err = gmail::search(&g, "", 5).await.unwrap_err().to_string();
    assert!(err.contains("connect again"), "{err}");
    assert!(!g.is_connected());
}
