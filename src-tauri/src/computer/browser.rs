//! Browser control through the Chrome DevTools Protocol.
//!
//! IGRIS starts its own Chromium-based browser (Chrome or Edge) with a separate
//! profile and a DevTools port bound to 127.0.0.1, then works on the page's DOM:
//! a snapshot lists the interactive elements with stable references, clicks
//! and typing go to those elements as trusted in-page input (the user's real
//! mouse and keyboard are not used), and every action reports what the page
//! looks like afterwards. It does not attach to the user's own browser
//! profile; other browser windows are still operated through UI Automation.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::cdp::{http_json, Cdp};

/// Elements listed per snapshot.
pub const MAX_ELEMENTS: usize = 150;
/// Visible page text included in a snapshot.
pub const MAX_TEXT: usize = 2_000;

/// Chromium-based browsers IGRIS can drive, most preferred first.
pub fn find_browser() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("IGRIS_BROWSER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
            if let Ok(base) = std::env::var(var) {
                candidates.push(std::path::Path::new(&base).join(r"Google\Chrome\Application\chrome.exe"));
                candidates.push(std::path::Path::new(&base).join(r"Microsoft\Edge\Application\msedge.exe"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        candidates.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
        candidates.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge"] {
            if let Some(path) = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())) {
                candidates.push(path);
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// The page right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct PageInfo {
    pub url: String,
    pub title: String,
    /// `document.readyState` is "complete".
    pub loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, Serialize)]
pub struct PageElement {
    /// Reference for browser_click / browser_type (valid until the next snapshot).
    #[serde(rename = "ref")]
    pub reference: u32,
    pub tag: String,
    pub role: String,
    pub name: String,
    /// Current value of a text field (never of password fields).
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub href: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub page: PageInfo,
    pub elements: Vec<PageElement>,
    /// Elements matching before the cap.
    pub total: usize,
    /// Visible text, clipped.
    pub text: String,
}

/// Collects interactive, visible elements and tags them with references.
const SNAPSHOT_JS: &str = r#"(() => {
  const find = (window.__igrisFind || '').toLowerCase();
  const sel = 'a[href],button,input,textarea,select,summary,[role=button],[role=link],[role=textbox],[role=searchbox],[role=checkbox],[role=tab],[role=menuitem],[role=combobox],[contenteditable=""],[contenteditable=true]';
  const out = []; let total = 0; let n = 0;
  document.querySelectorAll('[data-igris-ref]').forEach(e => e.removeAttribute('data-igris-ref'));
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect(); const st = getComputedStyle(el);
    if (r.width < 2 || r.height < 2 || st.visibility === 'hidden' || st.display === 'none') continue;
    if (el.type === 'hidden') continue;
    const label = el.labels && el.labels[0] ? el.labels[0].innerText : '';
    const name = (el.getAttribute('aria-label') || label || el.innerText || el.value && el.type !== 'password' && (el.type === 'submit' || el.type === 'button') && el.value || el.placeholder || el.title || el.name || el.alt || '').trim().replace(/\s+/g, ' ').slice(0, 120);
    if (find && !name.toLowerCase().includes(find)) continue;
    total++;
    if (out.length >= __MAX__) continue;
    n++; el.setAttribute('data-igris-ref', String(n));
    const tag = el.tagName.toLowerCase();
    const field = tag === 'input' || tag === 'textarea' || el.isContentEditable;
    out.push({ ref: n, tag, role: el.getAttribute('role') || (tag === 'input' ? (el.type || 'text') : tag), name,
      value: field && el.type !== 'password' ? (el.isContentEditable ? el.innerText : el.value || '').slice(0, 300) : null,
      href: tag === 'a' ? el.href.slice(0, 200) : null, disabled: !!el.disabled });
  }
  return { url: location.href, title: document.title, loaded: document.readyState === 'complete', elements: out, total,
    text: (document.body ? document.body.innerText : '').replace(/\s+\n/g, '\n').slice(0, __TEXT__) };
})()"#;

fn page_of(v: &Value) -> PageInfo {
    PageInfo {
        url: v["url"].as_str().unwrap_or_default().into(),
        title: v["title"].as_str().unwrap_or_default().into(),
        loaded: v["loaded"].as_bool().unwrap_or(false),
    }
}

const PAGE_JS: &str = "({ url: location.href, title: document.title, loaded: document.readyState === 'complete' })";

/// Only web pages (and a blank page) are opened.
pub fn check_url(url: &str) -> Result<(), String> {
    let lower = url.trim().to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower == "about:blank" {
        Ok(())
    } else {
        Err("Only http(s) web pages can be opened in the browser.".into())
    }
}

struct Live {
    child: Option<Child>,
    cdp: Cdp,
}

/// IGRIS's browser: started on first use, reused afterwards.
pub struct Browser {
    profile: PathBuf,
    live: Mutex<Option<Live>>,
}

/// Longest wait for a launched browser to open its DevTools endpoint.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(45);

impl Browser {
    pub fn new(profile: PathBuf) -> Self {
        Self { profile, live: Mutex::new(None) }
    }

    pub fn available() -> bool {
        find_browser().is_some()
    }

    async fn launch(&self) -> Result<Live, String> {
        let exe = find_browser().ok_or("No Chrome or Edge browser was found on this computer.")?;
        std::fs::create_dir_all(&self.profile).map_err(|e| format!("Couldn't prepare the browser profile: {e}"))?;
        let port_file = self.profile.join("DevToolsActivePort");
        let _ = std::fs::remove_file(&port_file);
        let mut cmd = Command::new(&exe);
        cmd.arg("--remote-debugging-port=0").arg("--remote-debugging-address=127.0.0.1").arg(format!("--user-data-dir={}", self.profile.display())).args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--new-window",
            "about:blank",
        ]);
        // Test and CI switches only (never set by IGRIS itself).
        if std::env::var("IGRIS_BROWSER_HEADLESS").is_ok() {
            cmd.arg("--headless=new");
        }
        if std::env::var("IGRIS_BROWSER_NO_SANDBOX").is_ok() {
            cmd.arg("--no-sandbox");
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start the browser: {e}"))?;
        // The browser writes its DevTools port to the profile once it's listening.
        // A first start with a fresh profile can be slow (profile creation, virus
        // scanning, a busy or slow disk), so allow well beyond a typical start.
        let started = Instant::now();
        let deadline = started + LAUNCH_TIMEOUT;
        let port: u16 = loop {
            if let Some(p) = std::fs::read_to_string(&port_file).ok().and_then(|s| s.lines().next().and_then(|l| l.trim().parse().ok())) {
                break p;
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("The browser exited right away ({status}). It may already be running with this profile."));
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                return Err(format!("The browser didn't start its DevTools endpoint in time (still starting after {} s).", started.elapsed().as_secs()));
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        };
        let cdp = connect_page(port).await?;
        Ok(Live { child: Some(child), cdp })
    }

    /// Run `f` with a live connection, starting the browser if needed.
    async fn with<T>(
        &self,
        f: impl for<'a> FnOnce(&'a mut Cdp) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send + 'a>>,
    ) -> Result<T, String> {
        let mut g = self.live.lock().await;
        let alive = match g.as_mut() {
            Some(l) => still_running(l.child.as_mut()) && l.cdp.eval("1").await.is_ok(),
            None => false,
        };
        if !alive {
            if let Some(mut old) = g.take() {
                if let Some(c) = old.child.as_mut() {
                    let _ = c.kill();
                }
            }
            *g = Some(self.launch().await?);
        }
        let live = g.as_mut().ok_or("The browser isn't running.")?;
        f(&mut live.cdp).await
    }

    pub async fn navigate(&self, url: &str) -> Result<PageInfo, String> {
        check_url(url)?;
        let url = url.to_string();
        self.with(move |cdp| {
            Box::pin(async move {
                cdp.call("Page.enable", json!({})).await?;
                let r = cdp.call("Page.navigate", json!({ "url": url })).await?;
                if let Some(e) = r["errorText"].as_str().filter(|e| !e.is_empty()) {
                    return Err(format!("The page couldn't be opened: {e}"));
                }
                wait_loaded(cdp, Duration::from_secs(20)).await
            })
        })
        .await
    }

    pub async fn page(&self) -> Result<PageInfo, String> {
        self.with(|cdp| Box::pin(async move { Ok(page_of(&cdp.eval(PAGE_JS).await?)) })).await
    }

    pub async fn snapshot(&self, find: &str) -> Result<Snapshot, String> {
        let js = SNAPSHOT_JS.replace("__MAX__", &MAX_ELEMENTS.to_string()).replace("__TEXT__", &MAX_TEXT.to_string());
        let find = serde_json::to_string(find).unwrap_or_else(|_| "\"\"".into());
        self.with(move |cdp| {
            Box::pin(async move {
                cdp.eval(&format!("window.__igrisFind = {find}; 0")).await?;
                let v = cdp.eval(&js).await?;
                let elements: Vec<PageElement> = serde_json::from_value(v["elements"].clone()).map_err(|e| format!("Unexpected page data: {e}"))?;
                Ok(Snapshot {
                    page: page_of(&v),
                    total: v["total"].as_u64().unwrap_or(0) as usize,
                    text: v["text"].as_str().unwrap_or_default().to_string(),
                    elements,
                })
            })
        })
        .await
    }

    /// The element `reference` from the last snapshot: its name and centre (CSS pixels), scrolled into view.
    async fn locate(cdp: &mut Cdp, reference: u32) -> Result<(String, f64, f64, bool), String> {
        let v = cdp
            .eval(&format!(
                "(() => {{ const el = document.querySelector('[data-igris-ref=\"{reference}\"]'); if (!el) return null; \
                 el.scrollIntoView({{block: 'center', inline: 'center'}}); const r = el.getBoundingClientRect(); \
                 const name = (el.getAttribute('aria-label') || el.innerText || el.value || el.placeholder || '').trim().slice(0, 120); \
                 const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2); \
                 return {{ name, x: r.left + r.width / 2, y: r.top + r.height / 2, covered: !!top && top !== el && !el.contains(top) && !top.contains(el) }}; }})()"
            ))
            .await?;
        if v.is_null() {
            return Err(format!("Element [{reference}] is no longer on the page. Take a new browser_snapshot."));
        }
        Ok((
            v["name"].as_str().unwrap_or_default().to_string(),
            v["x"].as_f64().unwrap_or(0.0),
            v["y"].as_f64().unwrap_or(0.0),
            v["covered"].as_bool().unwrap_or(false),
        ))
    }

    /// The name of element `reference` (for safety checks before acting).
    pub async fn element_name(&self, reference: u32) -> Result<String, String> {
        self.with(move |cdp| Box::pin(async move { Ok(Self::locate(cdp, reference).await?.0) })).await
    }

    /// Click an element with a trusted in-page mouse event; returns the page afterwards.
    pub async fn click(&self, reference: u32) -> Result<(String, PageInfo), String> {
        self.with(move |cdp| {
            Box::pin(async move {
                let (name, x, y, covered) = Self::locate(cdp, reference).await?;
                if covered {
                    return Err(format!("Element [{reference}] \"{name}\" is covered by something else (a dialog or banner?). Take a new browser_snapshot."));
                }
                for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
                    cdp.call("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 })).await?;
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
                let page = wait_loaded(cdp, Duration::from_secs(15)).await?;
                Ok((name, page))
            })
        })
        .await
    }

    /// Type into a field (replacing its text when `clear`); returns what the field now holds.
    pub async fn type_text(&self, reference: u32, text: &str, clear: bool) -> Result<(String, Option<String>), String> {
        let text = text.to_string();
        self.with(move |cdp| {
            Box::pin(async move {
                let (name, _, _, _) = Self::locate(cdp, reference).await?;
                let ok = cdp
                    .eval(&format!(
                        "(() => {{ const el = document.querySelector('[data-igris-ref=\"{reference}\"]'); if (!el) return false; el.focus(); \
                         if ({clear}) {{ if (el.select) el.select(); else document.execCommand('selectAll'); }} \
                         return document.activeElement === el || el.contains(document.activeElement); }})()"
                    ))
                    .await?;
                if ok != Value::Bool(true) {
                    return Err(format!("Element [{reference}] \"{name}\" can't take typing focus."));
                }
                cdp.call("Input.insertText", json!({ "text": text })).await?;
                let value = cdp
                    .eval(&format!(
                        "(() => {{ const el = document.querySelector('[data-igris-ref=\"{reference}\"]'); if (!el || el.type === 'password') return null; \
                         return el.isContentEditable ? el.innerText : el.value; }})()"
                    ))
                    .await?;
                Ok((name, value.as_str().map(str::to_string)))
            })
        })
        .await
    }

    /// Close IGRIS's browser (it is IGRIS's own instance and profile).
    pub async fn close(&self) {
        if let Some(mut l) = self.live.lock().await.take() {
            let _ = l.cdp.call("Browser.close", json!({})).await;
            if let Some(c) = l.child.as_mut() {
                let _ = c.wait();
            }
        }
    }
}

/// No child (an attached browser) or a child that is still running.
fn still_running(child: Option<&mut Child>) -> bool {
    match child {
        None => true,
        Some(c) => matches!(c.try_wait(), Ok(None)),
    }
}

async fn wait_loaded(cdp: &mut Cdp, limit: Duration) -> Result<PageInfo, String> {
    let deadline = Instant::now() + limit;
    loop {
        // A navigation in progress briefly has no execution context; retry.
        if let Ok(v) = cdp.eval(PAGE_JS).await {
            let p = page_of(&v);
            if p.loaded || Instant::now() > deadline {
                return Ok(p);
            }
        } else if Instant::now() > deadline {
            return Err("The page didn't respond.".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Connect to the first page target (waiting for one to exist).
async fn connect_page(port: u16) -> Result<Cdp, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(targets) = http_json(port, "/json/list").await {
            if let Some(ws) = targets.as_array().and_then(|a| a.iter().find(|t| t["type"] == "page")).and_then(|t| t["webSocketDebuggerUrl"].as_str()) {
                return Cdp::connect(ws).await;
            }
        }
        if Instant::now() > deadline {
            return Err("The browser has no page to control.".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serve one HTML page on every request.
    async fn serve(html: &'static str) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = s.read(&mut buf).await;
                    let req = String::from_utf8_lossy(&buf);
                    let body = if req.starts_with("GET /done") { "<html><head><title>Done</title></head><body>Sent!</body></html>" } else { html };
                    let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn only_web_pages_are_opened() {
        assert!(check_url("https://example.com").is_ok());
        assert!(check_url("about:blank").is_ok());
        for bad in ["file:///C:/Windows/win.ini", "javascript:alert(1)", "chrome://settings", "data:text/html,hi"] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    /// Drives a real Chromium when one is installed (skipped otherwise).
    #[tokio::test(flavor = "multi_thread")]
    async fn drives_a_real_chromium_through_devtools() {
        if find_browser().is_none() {
            eprintln!("no Chromium-based browser found; skipping");
            return;
        }
        std::env::set_var("IGRIS_BROWSER_HEADLESS", "1");
        #[cfg(unix)]
        std::env::set_var("IGRIS_BROWSER_NO_SANDBOX", "1");
        let url = serve(
            r#"<html><head><title>IGRIS test form</title></head><body>
            <h1>Contact</h1>
            <label for="n">Your name</label><input id="n" name="name" value="">
            <input type="password" aria-label="Password" value="secret">
            <button id="go" onclick="document.title='Clicked '+document.getElementById('n').value">Preview</button>
            <a href="/done">Finish</a>
            <div style="display:none"><button>Hidden</button></div>
            </body></html>"#,
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let b = Browser::new(dir.path().join("profile"));
        let p = b.navigate(&url).await.expect("navigate");
        assert_eq!((p.title.as_str(), p.loaded), ("IGRIS test form", true));
        let snap = b.snapshot("").await.unwrap();
        let names: Vec<&str> = snap.elements.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Your name") && names.contains(&"Preview") && names.contains(&"Finish"), "{names:?}");
        assert!(!names.contains(&"Hidden"), "invisible elements aren't listed");
        let pw = snap.elements.iter().find(|e| e.name == "Password").unwrap();
        assert_eq!(pw.value, None, "password values are never read");
        assert!(snap.text.contains("Contact"));

        let field = snap.elements.iter().find(|e| e.name == "Your name").unwrap().reference;
        let (_, value) = b.type_text(field, "Ada Lovelace — ✓", true).await.unwrap();
        assert_eq!(value.as_deref(), Some("Ada Lovelace — ✓"), "typed text reads back");
        let button = snap.elements.iter().find(|e| e.name == "Preview").unwrap().reference;
        let (name, page) = b.click(button).await.unwrap();
        assert_eq!((name.as_str(), page.title.as_str()), ("Preview", "Clicked Ada Lovelace — ✓"), "a trusted click ran the page's handler");
        let link = b.snapshot("finish").await.unwrap().elements[0].reference;
        let (_, page) = b.click(link).await.unwrap();
        assert_eq!((page.title.as_str(), page.url.ends_with("/done")), ("Done", true), "navigation is observed");
        assert!(b.click(999).await.unwrap_err().contains("no longer on the page"));
        b.close().await;
    }
}
