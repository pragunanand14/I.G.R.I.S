//! OAuth for installed apps (RFC 8252): PKCE, a one-shot listener on
//! 127.0.0.1 for the redirect, the code exchange, and token refresh.

use std::time::Duration;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{Google, Opener, SCOPES};
use crate::error::{AppError, AppResult};

/// How long the user has to finish signing in.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// `application/x-www-form-urlencoded` (also used for query strings).
pub(crate) fn form(pairs: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse("http://x/").expect("static url");
    u.query_pairs_mut().extend_pairs(pairs.iter().copied());
    u.query().unwrap_or_default().to_string()
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) struct Granted {
    pub access_token: String,
    pub refresh_token: String,
    pub lifetime: Duration,
    pub scopes: String,
}

/// Run the browser sign-in and exchange the code for tokens.
pub(crate) async fn sign_in(g: &Google, client_id: &str, secret: &str, open: &Opener) -> AppResult<Granted> {
    let verifier = b64url(crate::device::identity::random_hex(48).as_bytes());
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let state = crate::device::identity::random_hex(16);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| AppError::internal(format!("Couldn't start the sign-in listener: {e}")))?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| AppError::internal(e.to_string()))?.port());
    let scope = SCOPES.join(" ");
    let url = super::with_query(
        &g.ep.auth,
        &[
            ("client_id", client_id),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", &scope),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            // A refresh token, and the consent screen every time (so it's always issued).
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    );
    open(&url).map_err(|e| AppError::validation(format!("Couldn't open the browser for Google sign-in: {e}")))?;
    tracing::info!(event = "GOOGLE_SIGN_IN_STARTED");

    let code = tokio::time::timeout(SIGN_IN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| AppError::validation("Google sign-in wasn't finished within 5 minutes. Try again."))??;

    let body = form(&[
        ("client_id", client_id),
        ("client_secret", secret),
        ("code", &code),
        ("code_verifier", &verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", &redirect),
    ]);
    let v = token_call(g, body).await.map_err(|(_, e)| e)?;
    let scopes = v["scope"].as_str().unwrap_or_default().to_string();
    for needed in &SCOPES[..2] {
        if !scopes.split(' ').any(|s| s == *needed) {
            return Err(AppError::validation("Google didn't give IGRIS access to both Gmail and Calendar. Connect again and tick every box on Google's page."));
        }
    }
    Ok(Granted {
        access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
        refresh_token: v["refresh_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| AppError::validation("Google didn't send a long-term sign-in. Try connecting again."))?
            .to_string(),
        lifetime: Duration::from_secs(v["expires_in"].as_u64().unwrap_or(3600)),
        scopes,
    })
}

/// Accept connections on the redirect listener until Google's redirect
/// (with our `state`) arrives; answer it with a short page.
async fn wait_for_code(listener: &tokio::net::TcpListener, state: &str) -> AppResult<String> {
    loop {
        let (mut sock, _) = listener.accept().await.map_err(|e| AppError::internal(e.to_string()))?;
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut buf)).await.ok().and_then(|r| r.ok()).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let target = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/").to_string();
        let params: Vec<(String, String)> = reqwest::Url::parse(&format!("http://127.0.0.1{target}"))
            .map(|u| u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect())
            .unwrap_or_default();
        let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let (page, outcome) = match (get("state"), get("code"), get("error")) {
            (Some(s), Some(code), _) if s == state => ("IGRIS is connected to your Google account. You can close this tab.", Some(Ok(code))),
            (Some(s), _, Some(err)) if s == state => (
                "Google sign-in was cancelled. You can close this tab.",
                Some(Err(AppError::validation(if err == "access_denied" {
                    "Google sign-in was cancelled.".to_string()
                } else {
                    format!("Google sign-in failed ({err}).")
                }))),
            ),
            // Anything else (favicon, a stray request, a wrong state) is ignored.
            _ => ("", None),
        };
        let html = format!("<!doctype html><meta charset=utf-8><title>IGRIS</title><body style=\"font-family:sans-serif;padding:3em;text-align:center\"><h2>{page}</h2></body>");
        let status = if outcome.is_some() { "200 OK" } else { "404 Not Found" };
        let resp = format!("HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}", html.len());
        let _ = sock.write_all(resp.as_bytes()).await;
        let _ = sock.shutdown().await;
        if let Some(o) = outcome {
            return o;
        }
    }
}

pub(crate) enum RefreshError {
    /// The user removed IGRIS's access (or it expired): sign in again.
    Revoked,
    Other(AppError),
}

pub(crate) async fn refresh(g: &Google, client_id: &str, secret: &str, refresh_token: &str) -> Result<(String, Duration), RefreshError> {
    let body = form(&[("client_id", client_id), ("client_secret", secret), ("refresh_token", refresh_token), ("grant_type", "refresh_token")]);
    match token_call(g, body).await {
        Ok(v) => Ok((v["access_token"].as_str().unwrap_or_default().to_string(), Duration::from_secs(v["expires_in"].as_u64().unwrap_or(3600)))),
        Err((Some(err), _)) if err == "invalid_grant" => Err(RefreshError::Revoked),
        Err((_, e)) => Err(RefreshError::Other(e)),
    }
}

/// POST to the token endpoint. Errors carry Google's error code, if any.
async fn token_call(g: &Google, body: String) -> Result<Value, (Option<String>, AppError)> {
    let res = g
        .http
        .post(&g.ep.token)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|_| (None, AppError::validation("Couldn't reach Google — check the internet connection.")))?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if ok && v["access_token"].is_string() {
        return Ok(v);
    }
    let code = v["error"].as_str().map(str::to_string);
    let msg = match code.as_deref() {
        Some("invalid_client") => "Google didn't recognise IGRIS's OAuth client. Check GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET.".to_string(),
        Some("invalid_grant") => "Google sign-in expired or was removed. Connect again in Settings → Accounts.".to_string(),
        Some(c) => format!("Google sign-in failed ({c})."),
        None => "Google sign-in failed.".to_string(),
    };
    Err((code, AppError::validation(msg)))
}
