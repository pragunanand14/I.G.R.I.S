//! Gmail: search, read, draft, send a draft, organise.
//!
//! Sending is a separate step from drafting so the user sees the exact email
//! (recipient, subject, text) on the approval prompt before it goes.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};

use super::{seg, Google};
use crate::error::{AppError, AppResult};

/// Text of one email given to the model, at most.
const MAX_BODY_CHARS: usize = 15_000;
pub const MAX_RESULTS: u32 = 25;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub unread: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub summary: Summary,
    pub to: String,
    pub cc: String,
    pub body: String,
    message_id: String,
    references: String,
}

/// A draft IGRIS made, remembered so the send prompt can show exactly what goes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
}

fn drafts() -> &'static Mutex<HashMap<String, Draft>> {
    static D: OnceLock<Mutex<HashMap<String, Draft>>> = OnceLock::new();
    D.get_or_init(Default::default)
}

/// A draft made in this session, for the send prompt.
pub fn known_draft(id: &str) -> Option<Draft> {
    drafts().lock().ok()?.get(id).cloned()
}

fn header(payload: &Value, name: &str) -> String {
    payload["headers"]
        .as_array()
        .and_then(|h| h.iter().find(|x| x["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))))
        .and_then(|x| x["value"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn summary(v: &Value) -> Summary {
    let p = &v["payload"];
    Summary {
        id: v["id"].as_str().unwrap_or_default().into(),
        thread_id: v["threadId"].as_str().unwrap_or_default().into(),
        from: header(p, "From"),
        subject: header(p, "Subject"),
        date: header(p, "Date"),
        snippet: html_unescape(v["snippet"].as_str().unwrap_or_default()),
        unread: v["labelIds"].as_array().is_some_and(|l| l.iter().any(|x| x == "UNREAD")),
    }
}

fn html_unescape(s: &str) -> String {
    s.replace("&#39;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn decode(data: &str) -> Option<String> {
    let d = data.trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(d).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The readable text of a message: its text/plain part, else its HTML as text.
fn body_text(payload: &Value) -> String {
    fn find(p: &Value, mime: &str) -> Option<String> {
        if p["mimeType"].as_str() == Some(mime) {
            if let Some(t) = p["body"]["data"].as_str().and_then(decode) {
                return Some(t);
            }
        }
        p["parts"].as_array()?.iter().find_map(|c| find(c, mime))
    }
    if let Some(t) = find(payload, "text/plain") {
        return t;
    }
    find(payload, "text/html").map(|h| html2text::from_read(h.as_bytes(), 100).unwrap_or_default()).unwrap_or_default()
}

/// Search with Gmail's own syntax ("is:unread", "from:rahul", "newer_than:2d", …).
pub async fn search(g: &Google, query: &str, max: u32) -> AppResult<Vec<Summary>> {
    let max = max.clamp(1, MAX_RESULTS).to_string();
    let list = g.get_json(&format!("{}/messages", g.ep.gmail), &[("q", query), ("maxResults", &max)]).await?;
    let mut out = Vec::new();
    for m in list["messages"].as_array().into_iter().flatten() {
        let Some(id) = m["id"].as_str() else { continue };
        let v = g
            .get_json(
                &format!("{}/messages/{}", g.ep.gmail, seg(id)),
                &[("format", "metadata"), ("metadataHeaders", "From"), ("metadataHeaders", "Subject"), ("metadataHeaders", "Date")],
            )
            .await?;
        out.push(summary(&v));
    }
    Ok(out)
}

pub async fn read(g: &Google, id: &str) -> AppResult<Message> {
    let v = g.get_json(&format!("{}/messages/{}", g.ep.gmail, seg(id)), &[("format", "full")]).await?;
    let p = &v["payload"];
    let mut body = body_text(p);
    if body.chars().count() > MAX_BODY_CHARS {
        body = body.chars().take(MAX_BODY_CHARS).collect::<String>() + "\n[… rest of the email cut]";
    }
    Ok(Message {
        summary: summary(&v),
        to: header(p, "To"),
        cc: header(p, "Cc"),
        body,
        message_id: header(p, "Message-ID"),
        references: header(p, "References"),
    })
}

/// Addresses and subject must be one line (no header injection).
fn one_line(s: &str, what: &str) -> AppResult<String> {
    if s.contains(['\r', '\n']) {
        return Err(AppError::validation(format!("The {what} can't contain line breaks.")));
    }
    Ok(s.trim().to_string())
}

fn encode_subject(s: &str) -> String {
    if s.is_ascii() {
        s.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode(s))
    }
}

pub struct NewDraft<'a> {
    pub to: &'a str,
    pub cc: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    /// Reply to this message (same thread, quoted headers).
    pub reply_to: Option<&'a str>,
}

/// Make a draft in the user's Gmail (nothing is sent).
pub async fn draft(g: &Google, d: NewDraft<'_>) -> AppResult<Draft> {
    let mut to = one_line(d.to, "recipient")?;
    let cc = one_line(d.cc, "Cc")?;
    let mut subject = one_line(d.subject, "subject")?;
    let mut extra = String::new();
    let mut thread = None;
    if let Some(id) = d.reply_to.filter(|s| !s.trim().is_empty()) {
        let orig = read(g, id).await?;
        if to.is_empty() {
            to = orig.summary.from.clone();
        }
        if subject.is_empty() {
            subject = orig.summary.subject.clone();
        }
        if !subject.to_lowercase().starts_with("re:") {
            subject = format!("Re: {subject}");
        }
        if !orig.message_id.is_empty() {
            extra = format!("In-Reply-To: {}\r\nReferences: {} {}\r\n", orig.message_id, orig.references, orig.message_id);
        }
        thread = Some(orig.summary.thread_id);
    }
    if to.is_empty() {
        return Err(AppError::validation("Say who the email is to."));
    }
    let cc_line = if cc.is_empty() { String::new() } else { format!("Cc: {cc}\r\n") };
    let body64 = base64::engine::general_purpose::STANDARD.encode(d.body.replace("\r\n", "\n").replace('\n', "\r\n"));
    let raw = format!(
        "To: {to}\r\n{cc_line}Subject: {}\r\n{extra}MIME-Version: 1.0\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{body64}",
        encode_subject(&subject)
    );
    let mut message = json!({ "raw": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw) });
    if let Some(t) = thread {
        message["threadId"] = json!(t);
    }
    let v = g.post_json(&format!("{}/drafts", g.ep.gmail), &[], &json!({ "message": message })).await?;
    let id = v["id"].as_str().ok_or_else(|| AppError::internal("Gmail didn't return the draft"))?.to_string();
    let draft = Draft { id: id.clone(), to, cc, subject, body: d.body.to_string() };
    if let Ok(mut m) = drafts().lock() {
        m.insert(id, draft.clone());
    }
    tracing::info!(event = "GMAIL_DRAFT_CREATED");
    Ok(draft)
}

/// Send a draft. Returns the sent message's id.
pub async fn send_draft(g: &Google, draft_id: &str) -> AppResult<String> {
    let v = g.post_json(&format!("{}/drafts/send", g.ep.gmail), &[], &json!({ "id": draft_id })).await?;
    if let Ok(mut m) = drafts().lock() {
        m.remove(draft_id);
    }
    tracing::info!(event = "GMAIL_SENT");
    Ok(v["id"].as_str().unwrap_or_default().to_string())
}

/// archive, mark_read, mark_unread, star, unstar, or trash (recoverable for 30 days).
pub async fn organise(g: &Google, id: &str, action: &str) -> AppResult<&'static str> {
    if action == "trash" {
        g.post_json(&format!("{}/messages/{}/trash", g.ep.gmail, seg(id)), &[], &json!({})).await?;
        return Ok("Moved to Bin (recoverable for 30 days)");
    }
    let (add, remove, done): (&[&str], &[&str], &'static str) = match action {
        "archive" => (&[], &["INBOX"], "Archived"),
        "mark_read" => (&[], &["UNREAD"], "Marked as read"),
        "mark_unread" => (&["UNREAD"], &[], "Marked as unread"),
        "star" => (&["STARRED"], &[], "Starred"),
        "unstar" => (&[], &["STARRED"], "Unstarred"),
        _ => return Err(AppError::validation("Unknown action.")),
    };
    g.post_json(&format!("{}/messages/{}/modify", g.ep.gmail, seg(id)), &[], &json!({ "addLabelIds": add, "removeLabelIds": remove })).await?;
    Ok(done)
}
