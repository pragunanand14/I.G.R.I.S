//! The user's Google account: Gmail and Google Calendar.
//!
//! On a computer, connecting uses Google's OAuth for installed apps: the
//! user's own OAuth client (pasted once in Settings → Accounts, or
//! GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET in `.env`; see docs/GOOGLE.md),
//! PKCE, and a one-shot listener on 127.0.0.1 for the redirect.
//!
//! On the phone, the Google account already on the phone signs in through
//! Google Play services' own account picker ([`NativeAuth`]): no client to
//! paste, and no refresh token kept — Play services hands out a fresh access
//! token whenever one is needed. The refresh token is sealed by the device's key protector (DPAPI
//! on Windows, the Android Keystore) and stored in the app's database; access
//! tokens live in memory only. Nothing about the account reaches the UI but
//! the email address. Email and calendar content is untrusted data.

pub mod calendar;
pub mod gmail;
mod oauth;
pub mod watcher;

#[cfg(test)]
pub(crate) mod tests;

use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::db::Database;
use crate::device::KeyProtector;
use crate::error::{AppError, AppResult};

/// What IGRIS asks Google for: read/organise/draft/send mail, manage events, and who the user is.
pub const SCOPES: &[&str] = &["https://www.googleapis.com/auth/gmail.modify", "https://www.googleapis.com/auth/calendar.events", "openid", "email"];

/// Where Google lives (tests point these at a fake).
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub gmail: String,
    pub calendar: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            gmail: "https://gmail.googleapis.com/gmail/v1/users/me".into(),
            calendar: "https://www.googleapis.com/calendar/v3".into(),
        }
    }
}

/// The OAuth client (id, secret) from the user's configuration, read when needed.
pub type Credentials = Arc<dyn Fn() -> Option<(String, String)> + Send + Sync>;

/// The phone's own Google sign-in (Google Play services): an access token for the scopes, showing Google's
/// account picker when `interactive` and needed. `Ok(None)`: the user has to be asked. Blocking.
pub type NativeAuth = Arc<dyn Fn(&[&str], bool) -> Result<Option<String>, String> + Send + Sync>;

/// Marks an account signed in through [`NativeAuth`] (stored in `scopes`; there's no refresh token).
const NATIVE: &str = "native";

/// Where the OAuth client pasted in Settings → Accounts is kept (sealed by the key protector).
const CLIENT_KEY: &str = "google.client";

/// How long a token from the phone's sign-in is used before asking for a fresh one (they last an hour).
const NATIVE_TOKEN_LIFE: Duration = Duration::from_secs(45 * 60);

/// Opens the sign-in page in the browser.
pub type Opener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Connecting can start: the phone's own sign-in, or an OAuth client (pasted or in `.env`).
    pub configured: bool,
    /// Signing in uses the phone's Google account picker.
    pub native: bool,
    pub connected: bool,
    pub email: Option<String>,
}

pub struct Google {
    db: Arc<Database>,
    protector: Arc<dyn KeyProtector>,
    creds: Credentials,
    pub(crate) ep: Endpoints,
    pub(crate) http: reqwest::Client,
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
    connecting: tokio::sync::Mutex<()>,
    native: Option<NativeAuth>,
}

const NOT_CONNECTED: &str = "Google isn't connected. The user can connect it in IGRIS → Settings → Accounts.";

impl Google {
    pub fn new(db: Arc<Database>, protector: Arc<dyn KeyProtector>, creds: Credentials) -> Self {
        Self::with_endpoints(db, protector, creds, Endpoints::default())
    }

    pub fn with_endpoints(db: Arc<Database>, protector: Arc<dyn KeyProtector>, creds: Credentials, ep: Endpoints) -> Self {
        let http = crate::net::client_builder().timeout(Duration::from_secs(30)).build().unwrap_or_default();
        Self { db, protector, creds, ep, http, token: tokio::sync::Mutex::new(None), connecting: tokio::sync::Mutex::new(()), native: None }
    }

    /// Sign in with the phone's own Google account picker instead of the browser.
    pub fn with_native(mut self, native: NativeAuth) -> Self {
        self.native = Some(native);
        self
    }

    /// The OAuth client: from the settings file, else the one pasted in Settings → Accounts.
    fn client(&self) -> Option<(String, String)> {
        (self.creds)().or_else(|| self.saved_client().ok().flatten())
    }

    fn saved_client(&self) -> AppResult<Option<(String, String)>> {
        use base64::Engine;
        let Some(sealed) = crate::settings::get_extra::<String>(&*self.db.conn()?, CLIENT_KEY)? else { return Ok(None) };
        let sealed = base64::engine::general_purpose::STANDARD.decode(sealed).map_err(|_| AppError::internal("The saved Google setup is corrupt."))?;
        let plain = self.protector.unprotect(&sealed).map_err(|e| AppError::internal(format!("Couldn't unlock the saved Google setup: {e}")))?;
        let v: Value = serde_json::from_slice(&plain).map_err(|_| AppError::internal("The saved Google setup is corrupt."))?;
        Ok(Some((v["id"].as_str().unwrap_or_default().to_string(), v["secret"].as_str().unwrap_or_default().to_string())))
    }

    /// Save the user's OAuth client (from the setup guide), sealed on this device.
    pub fn set_client(&self, id: &str, secret: &str) -> AppResult<()> {
        use base64::Engine;
        let (id, secret) = (id.trim(), secret.trim());
        if !id.ends_with(".apps.googleusercontent.com") || id.len() > 200 || id.chars().any(char::is_whitespace) {
            return Err(AppError::validation("That doesn't look like a Google Client ID (it ends in .apps.googleusercontent.com)."));
        }
        if secret.len() < 8 || secret.len() > 200 || secret.chars().any(char::is_whitespace) {
            return Err(AppError::validation("That doesn't look like a Google Client secret (it usually starts with GOCSPX-)."));
        }
        let plain = serde_json::json!({ "id": id, "secret": secret }).to_string();
        let sealed = self.protector.protect(plain.as_bytes()).map_err(AppError::internal)?;
        crate::settings::set_extra(&*self.db.conn()?, CLIENT_KEY, &base64::engine::general_purpose::STANDARD.encode(sealed))?;
        tracing::info!(event = "GOOGLE_CLIENT_SAVED");
        Ok(())
    }

    /// Forget the pasted OAuth client (the settings file's, if any, still applies).
    pub fn clear_client(&self) -> AppResult<()> {
        self.db.conn()?.execute("DELETE FROM settings WHERE key = ?1", [format!("x.{CLIENT_KEY}")])?;
        Ok(())
    }

    fn native_account(&self) -> AppResult<bool> {
        let scopes: Option<String> = self.db.conn()?.query_row("SELECT scopes FROM google_account WHERE id = 1", [], |r| r.get(0)).optional()?;
        Ok(scopes.as_deref() == Some(NATIVE))
    }

    async fn native_token(&self, interactive: bool) -> AppResult<Option<String>> {
        let native = self.native.clone().ok_or_else(|| AppError::validation(NOT_CONNECTED))?;
        tokio::task::spawn_blocking(move || native(SCOPES, interactive)).await.map_err(|e| AppError::internal(e.to_string()))?.map_err(AppError::validation)
    }

    pub fn status(&self) -> AppResult<Status> {
        let email: Option<String> = self.db.conn()?.query_row("SELECT email FROM google_account WHERE id = 1", [], |r| r.get(0)).optional()?;
        let native = self.native.is_some();
        Ok(Status { configured: native || self.client().is_some(), native, connected: email.is_some(), email })
    }

    pub fn is_connected(&self) -> bool {
        self.status().map(|s| s.connected).unwrap_or(false)
    }

    /// Sign in: opens Google's page in the browser and waits (up to 5 minutes)
    /// for the user to allow access. Returns the account's email.
    pub async fn connect(&self, open: &Opener) -> AppResult<String> {
        let _one = self.connecting.try_lock().map_err(|_| AppError::validation("Google sign-in is already open."))?;
        if self.native.is_some() {
            return self.connect_native().await;
        }
        let (id, secret) = self.client().ok_or_else(|| {
            AppError::validation("Google isn't set up on this computer yet: paste the Client ID and secret from the setup guide in Settings → Accounts.")
        })?;
        let granted = oauth::sign_in(self, &id, &secret, open).await?;
        *self.token.lock().await = Some((granted.access_token.clone(), Instant::now() + granted.lifetime));
        let profile = self.get_json(&format!("{}/profile", self.ep.gmail), &[]).await?;
        let email = profile["emailAddress"].as_str().unwrap_or("your Google account").to_string();
        let sealed = self.protector.protect(granted.refresh_token.as_bytes()).map_err(AppError::internal)?;
        self.db.conn()?.execute(
            "INSERT OR REPLACE INTO google_account (id, email, refresh_sealed, scopes, connected_at) VALUES (1, ?1, ?2, ?3, ?4)",
            params![email, sealed, granted.scopes, chrono::Utc::now().to_rfc3339()],
        )?;
        tracing::info!(event = "GOOGLE_CONNECTED");
        Ok(email)
    }

    /// The phone: Google's account picker, then remember who signed in (no refresh token — Play services
    /// keeps the grant).
    async fn connect_native(&self) -> AppResult<String> {
        let token = self.native_token(true).await?.ok_or_else(|| AppError::validation("Google sign-in was cancelled."))?;
        *self.token.lock().await = Some((token, Instant::now() + NATIVE_TOKEN_LIFE));
        let profile = match self.get_json(&format!("{}/profile", self.ep.gmail), &[]).await {
            Ok(p) => p,
            Err(e) => {
                *self.token.lock().await = None;
                return Err(e);
            }
        };
        let email = profile["emailAddress"].as_str().unwrap_or("your Google account").to_string();
        self.db.conn()?.execute(
            "INSERT OR REPLACE INTO google_account (id, email, refresh_sealed, scopes, connected_at) VALUES (1, ?1, ?2, ?3, ?4)",
            params![email, Vec::<u8>::new(), NATIVE, chrono::Utc::now().to_rfc3339()],
        )?;
        tracing::info!(event = "GOOGLE_CONNECTED", native = true);
        Ok(email)
    }

    /// Sign out: Google forgets IGRIS's access, and IGRIS forgets the account.
    pub async fn disconnect(&self) -> AppResult<()> {
        // The phone's sign-in isn't revoked here: Play services would keep handing out the dead token. IGRIS
        // just forgets the account; the grant stays with the phone's Google account (removable in its settings).
        if let Some(refresh) = self.refresh_token()? {
            let _ = self
                .http
                .post(&self.ep.revoke)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(oauth::form(&[("token", &refresh)]))
                .send()
                .await;
        }
        self.forget()?;
        tracing::info!(event = "GOOGLE_DISCONNECTED");
        Ok(())
    }

    fn forget(&self) -> AppResult<()> {
        let c = self.db.conn()?;
        c.execute("DELETE FROM google_account", [])?;
        if let Ok(mut t) = self.token.try_lock() {
            *t = None;
        }
        Ok(())
    }

    fn refresh_token(&self) -> AppResult<Option<String>> {
        let sealed: Option<Vec<u8>> = self.db.conn()?.query_row("SELECT refresh_sealed FROM google_account WHERE id = 1", [], |r| r.get(0)).optional()?;
        let Some(sealed) = sealed.filter(|s| !s.is_empty()) else { return Ok(None) };
        let plain = self.protector.unprotect(&sealed).map_err(|e| AppError::internal(format!("Couldn't unlock the Google sign-in: {e}")))?;
        Ok(Some(String::from_utf8(plain).map_err(|_| AppError::internal("The stored Google sign-in is corrupt."))?))
    }

    /// A valid access token, refreshed when it's about to expire.
    pub(crate) async fn access_token(&self) -> AppResult<String> {
        let mut cached = self.token.lock().await;
        if let Some((t, until)) = cached.as_ref() {
            if Instant::now() + Duration::from_secs(60) < *until {
                return Ok(t.clone());
            }
        }
        if self.native_account()? {
            // Play services gives a fresh token for the grant it remembers, without showing anything.
            return match self.native_token(false).await? {
                Some(t) => {
                    *cached = Some((t.clone(), Instant::now() + NATIVE_TOKEN_LIFE));
                    Ok(t)
                }
                None => Err(AppError::validation("Google needs the user to sign in again: IGRIS → Settings → Accounts → Connect Google.")),
            };
        }
        let refresh = self.refresh_token()?.ok_or_else(|| AppError::validation(NOT_CONNECTED))?;
        let (id, secret) = self.client().ok_or_else(|| AppError::validation("Google isn't set up on this device any more (see Settings → Accounts)."))?;
        match oauth::refresh(self, &id, &secret, &refresh).await {
            Ok((t, life)) => {
                *cached = Some((t.clone(), Instant::now() + life));
                Ok(t)
            }
            Err(oauth::RefreshError::Revoked) => {
                drop(cached);
                self.forget()?;
                Err(AppError::validation("Google access was removed or expired. The user needs to connect again in IGRIS → Settings → Accounts."))
            }
            Err(oauth::RefreshError::Other(e)) => Err(e),
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> AppResult<Value> {
        let token = self.access_token().await?;
        let res = req.bearer_auth(token).send().await.map_err(|_| AppError::validation("Couldn't reach Google — check the internet connection."))?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if status.is_success() {
            return Ok(if body.trim().is_empty() { Value::Null } else { serde_json::from_str(&body).unwrap_or(Value::Null) });
        }
        if status.as_u16() == 401 {
            *self.token.lock().await = None;
        }
        let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or_default();
        tracing::warn!(event = "GOOGLE_API_ERROR", status = status.as_u16());
        Err(AppError::validation(match status.as_u16() {
            401 => "Google didn't accept IGRIS's sign-in. Try again; if it keeps failing, reconnect in Settings → Accounts.".to_string(),
            403 if msg.to_lowercase().contains("scope") || msg.to_lowercase().contains("permission") => {
                "Google says IGRIS isn't allowed to do that. Reconnect in Settings → Accounts and tick every box on Google's page.".to_string()
            }
            404 => "Google couldn't find that (it may have been deleted).".to_string(),
            429 => "Google says IGRIS is asking too often. Try again in a minute.".to_string(),
            _ => format!("Google returned an error ({status}){}", if msg.is_empty() { String::new() } else { format!(": {msg}") }),
        }))
    }

    pub(crate) async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> AppResult<Value> {
        self.send(self.http.get(with_query(url, query))).await
    }

    pub(crate) async fn post_json(&self, url: &str, query: &[(&str, &str)], body: &Value) -> AppResult<Value> {
        self.send(self.http.post(with_query(url, query)).json(body)).await
    }

    pub(crate) async fn patch_json(&self, url: &str, query: &[(&str, &str)], body: &Value) -> AppResult<Value> {
        self.send(self.http.patch(with_query(url, query)).json(body)).await
    }

    pub(crate) async fn delete(&self, url: &str, query: &[(&str, &str)]) -> AppResult<()> {
        self.send(self.http.delete(with_query(url, query))).await.map(|_| ())
    }

    pub(crate) fn db(&self) -> &Arc<Database> {
        &self.db
    }
}

/// `url` with `query` appended, properly encoded.
pub(crate) fn with_query(url: &str, query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return url.to_string();
    }
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{}", oauth::form(query))
}

/// Path segment for an id that came from Google (or the model): encoded.
pub(crate) fn seg(id: &str) -> String {
    oauth::form(&[("", id)]).trim_start_matches('=').to_string()
}
