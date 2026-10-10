//! LG webOS TVs, controlled directly on the home network.
//!
//! The TV runs a WebSocket server ("SSAP") on port 3000 (`ws://`) and 3001
//! (`wss://`, required by newer firmware, with a self-signed certificate).
//! The first connection registers IGRIS: the TV shows a prompt and the user
//! presses Allow on the TV; the TV answers with a client key that later
//! connections present. The certificate seen at pairing is pinned (its
//! SHA-256 is stored) so a later connection can't be answered by another
//! machine on the network. Remote-control buttons go over a second socket the
//! TV hands out. Turning the TV on uses Wake-on-LAN (the TV must have "Turn on
//! via Wi-Fi" enabled).
//!
//! Protocol details follow the open-source aiowebostv client (Apache-2.0),
//! which documents LG's public SSAP endpoints.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::{AppError, AppResult};

pub const WS_PORT: u16 = 3000;
pub const WSS_PORT: u16 = 3001;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the user has to press Allow on the TV.
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(60);

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The permissions IGRIS asks the TV for (the same set LG's own companion apps use).
const PERMISSIONS: &[&str] = &[
    "APP_TO_APP",
    "CLOSE",
    "CONTROL_AUDIO",
    "CONTROL_DISPLAY",
    "CONTROL_INPUT_JOYSTICK",
    "CONTROL_INPUT_MEDIA_PLAYBACK",
    "CONTROL_INPUT_MEDIA_RECORDING",
    "CONTROL_INPUT_TEXT",
    "CONTROL_INPUT_TV",
    "CONTROL_MOUSE_AND_KEYBOARD",
    "CONTROL_POWER",
    "CONTROL_TV_SCREEN",
    "LAUNCH",
    "LAUNCH_WEBAPP",
    "READ_APP_STATUS",
    "READ_COUNTRY_INFO",
    "READ_CURRENT_CHANNEL",
    "READ_INPUT_DEVICE_LIST",
    "READ_INSTALLED_APPS",
    "READ_LGE_SDX",
    "READ_LGE_TV_INPUT_EVENTS",
    "READ_NETWORK_STATE",
    "READ_NOTIFICATIONS",
    "READ_POWER_STATE",
    "READ_RUNNING_APPS",
    "READ_SETTINGS",
    "READ_TV_CHANNEL_LIST",
    "READ_TV_CURRENT_TIME",
    "READ_UPDATE_INFO",
    "SEARCH",
    "TEST_OPEN",
    "TEST_PROTECTED",
    "TEST_SECURE",
    "UPDATE_FROM_REMOTE_APP",
    "WRITE_NOTIFICATION_ALERT",
    "WRITE_NOTIFICATION_TOAST",
    "WRITE_SETTINGS",
];

/// Remote-control buttons the TV accepts on its input socket.
pub const BUTTONS: &[&str] = &[
    "HOME", "BACK", "UP", "DOWN", "LEFT", "RIGHT", "ENTER", "MENU", "EXIT", "INFO", "RED", "GREEN", "YELLOW", "BLUE", "DASH", "0", "1", "2", "3", "4", "5",
    "6", "7", "8", "9",
];

/// What a connection learned about the TV, and what pairing produced.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub client_key: Option<String>,
    /// SHA-256 of the TLS certificate, when the TV was reached over `wss://`.
    pub cert_sha256: Option<String>,
}

/// Where to connect: the standard ports, or an explicit `host:port` (plain `ws://`, used by tests).
fn endpoints(host: &str) -> Vec<String> {
    if host.contains(':') && !host.starts_with('[') {
        vec![format!("ws://{host}")]
    } else {
        vec![format!("ws://{host}:{WS_PORT}"), format!("wss://{host}:{WSS_PORT}")]
    }
}

/// An open, registered connection to a TV.
pub struct Tv {
    ws: Ws,
    next_id: u64,
    pub session: Session,
}

impl Tv {
    /// Connect and register. With a stored key the TV accepts silently; without
    /// one it shows the "Allow" prompt and this waits up to [`PAIRING_TIMEOUT`].
    /// `pinned`: the certificate fingerprint stored at pairing (refuse anything else).
    pub async fn connect(host: &str, client_key: Option<&str>, pinned: Option<&str>) -> AppResult<Tv> {
        let mut last = String::new();
        for url in endpoints(host) {
            match open(&url, pinned).await {
                Ok((ws, cert_sha256)) => {
                    let mut tv = Tv { ws, next_id: 1, session: Session { client_key: client_key.map(str::to_string), cert_sha256 } };
                    tv.register(client_key).await?;
                    return Ok(tv);
                }
                Err(e) => last = e,
            }
        }
        Err(AppError::validation(format!("Couldn't reach the TV at {host} ({last}). Is it on and on the same Wi-Fi as this device?")))
    }

    async fn register(&mut self, client_key: Option<&str>) -> AppResult<()> {
        // Newer webOS versions want a system-info request before registration; its answer doesn't matter.
        let _ = self.send(json!({"id": "pre_sys_info", "type": "request", "uri": "ssap://system/getSystemInfo", "payload": {}})).await;
        let mut payload = json!({
            "forcePairing": false,
            "pairingType": "PROMPT",
            "manifest": { "appVersion": "1.1", "manifestVersion": 1, "permissions": PERMISSIONS },
        });
        if let Some(k) = client_key {
            payload["client-key"] = json!(k);
        }
        self.send(json!({"id": "register_0", "type": "register", "payload": payload})).await?;
        let deadline = tokio::time::Instant::now() + if client_key.is_some() { REQUEST_TIMEOUT } else { PAIRING_TIMEOUT };
        loop {
            let msg = tokio::time::timeout_at(deadline, self.recv()).await.map_err(|_| {
                AppError::validation(if client_key.is_some() {
                    "The TV didn't accept IGRIS. Pair it again."
                } else {
                    "Nobody pressed Allow on the TV in time."
                })
            })??;
            if msg["id"] != "register_0" {
                continue; // e.g. the pre-registration system info
            }
            match msg["type"].as_str() {
                Some("registered") => {
                    if let Some(k) = msg["payload"]["client-key"].as_str() {
                        self.session.client_key = Some(k.to_string());
                    }
                    return Ok(());
                }
                Some("response") => continue, // "PROMPT": the TV is asking the user
                Some("error") => {
                    let e = msg["error"].as_str().unwrap_or("refused");
                    return Err(AppError::validation(if e.contains("rejected") || e.contains("cancel") {
                        "Pairing was declined on the TV.".to_string()
                    } else {
                        format!("The TV refused IGRIS ({e}). Pair it again.")
                    }));
                }
                _ => continue,
            }
        }
    }

    async fn send(&mut self, v: Value) -> AppResult<()> {
        self.ws.send(WsMessage::Text(v.to_string().into())).await.map_err(|e| AppError::validation(format!("Lost the connection to the TV ({e}).")))
    }

    async fn recv(&mut self) -> AppResult<Value> {
        loop {
            match self.ws.next().await {
                Some(Ok(WsMessage::Text(t))) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        return Ok(v);
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None => return Err(AppError::validation("The TV closed the connection.")),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(AppError::validation(format!("Lost the connection to the TV ({e})."))),
            }
        }
    }

    /// One SSAP request (`uri` without the `ssap://` prefix); the TV's payload, or its error in plain words.
    pub async fn request(&mut self, uri: &str, payload: Value) -> AppResult<Value> {
        let id = format!("igris_{}", self.next_id);
        self.next_id += 1;
        self.send(json!({"id": id, "type": "request", "uri": format!("ssap://{uri}"), "payload": payload})).await?;
        let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
        loop {
            let msg = tokio::time::timeout_at(deadline, self.recv()).await.map_err(|_| AppError::validation("The TV didn't answer."))??;
            if msg["id"] != id.as_str() {
                continue;
            }
            if msg["type"] == "error" || msg["payload"]["returnValue"] == false {
                let why = msg["payload"]["errorText"].as_str().or(msg["error"].as_str()).unwrap_or("it refused");
                return Err(AppError::validation(format!("The TV couldn't do that ({why}).")));
            }
            return Ok(msg["payload"].clone());
        }
    }

    /// Press a remote-control button (see [`BUTTONS`]).
    pub async fn button(&mut self, name: &str) -> AppResult<()> {
        let name = name.trim().to_ascii_uppercase();
        if !BUTTONS.contains(&name.as_str()) {
            return Err(AppError::validation(format!("Unknown button \"{name}\". Buttons: {}.", BUTTONS.join(", "))));
        }
        let mut input = self.pointer().await?;
        input.send(&PointerEvent::Button(name)).await?;
        input.close().await;
        Ok(())
    }

    /// The TV's input channel (buttons, cursor, scrolling), kept open by the caller.
    pub async fn pointer(&mut self) -> AppResult<Pointer> {
        let res = self.request("com.webos.service.networkinput/getPointerInputSocket", json!({})).await?;
        let path = res["socketPath"].as_str().ok_or_else(|| AppError::validation("The TV didn't open its remote-control channel."))?.to_string();
        let (ws, _) = open(&path, self.session.cert_sha256.as_deref())
            .await
            .map_err(|e| AppError::validation(format!("Couldn't open the TV's remote-control channel ({e}).")))?;
        Ok(Pointer { ws })
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

/// One input on the TV's pointer channel. Movement is in screen pixels, relative.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PointerEvent {
    Move {
        dx: i32,
        dy: i32,
    },
    Click,
    Scroll {
        dx: i32,
        dy: i32,
    },
    #[serde(skip)]
    Button(String),
}

impl PointerEvent {
    /// The wire form (the input channel speaks "key:value" lines, ended by a blank line).
    pub fn wire(&self) -> String {
        // Guard against absurd values (a dropped batch shouldn't fling the cursor off screen).
        let c = |v: i32| v.clamp(-2000, 2000);
        match self {
            PointerEvent::Move { dx, dy } => format!("type:move\ndx:{}\ndy:{}\ndown:0\n\n", c(*dx), c(*dy)),
            PointerEvent::Click => "type:click\n\n".into(),
            PointerEvent::Scroll { dx, dy } => format!("type:scroll\ndx:{}\ndy:{}\n\n", c(*dx), c(*dy)),
            PointerEvent::Button(name) => format!("type:button\nname:{name}\n\n"),
        }
    }
}

/// The TV's open input channel.
pub struct Pointer {
    ws: Ws,
}

impl Pointer {
    pub async fn send(&mut self, e: &PointerEvent) -> AppResult<()> {
        self.ws.send(WsMessage::Text(e.wire().into())).await.map_err(|e| AppError::validation(format!("Lost the TV's remote-control channel ({e}).")))
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

/// Open a WebSocket; for `wss://`, accept the TV's self-signed certificate only
/// if it matches `pinned` (or, when nothing is pinned yet, remember it).
async fn open(url: &str, pinned: Option<&str>) -> Result<(Ws, Option<String>), String> {
    let seen = Arc::new(Mutex::new(None::<String>));
    let connector = url.starts_with("wss://").then(|| {
        let verifier =
            Arc::new(PinnedCert { pinned: pinned.map(str::to_string), seen: seen.clone(), provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()) });
        let config = rustls::ClientConfig::builder_with_provider(verifier.provider.clone())
            .with_safe_default_protocol_versions()
            .map(|b| b.dangerous().with_custom_certificate_verifier(verifier).with_no_client_auth());
        config.map(|c| tokio_tungstenite::Connector::Rustls(Arc::new(c)))
    });
    let connector = match connector {
        Some(Ok(c)) => Some(c),
        Some(Err(e)) => return Err(e.to_string()),
        None => None,
    };
    let fut = tokio_tungstenite::connect_async_tls_with_config(url, None, false, connector);
    match tokio::time::timeout(CONNECT_TIMEOUT, fut).await {
        Ok(Ok((ws, _))) => {
            let fp = seen.lock().map(|g| g.clone()).unwrap_or(None);
            Ok((ws, fp))
        }
        Ok(Err(e)) => {
            let s = e.to_string();
            Err(if s.contains("pinned") { "its certificate changed since pairing — pair it again if you replaced or reset the TV".into() } else { s })
        }
        Err(_) => Err("timed out".into()),
    }
}

/// Trust-on-first-use for the TV's self-signed certificate.
#[derive(Debug)]
struct PinnedCert {
    pinned: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

impl rustls::client::danger::ServerCertVerifier for PinnedCert {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fp = fingerprint(end_entity.as_ref());
        if let Some(p) = &self.pinned {
            if *p != fp {
                return Err(rustls::Error::General("certificate doesn't match the pinned one".into()));
            }
        }
        if let Ok(mut g) = self.seen.lock() {
            *g = Some(fp);
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Wake-on-LAN: the "magic packet" (6 × 0xFF, then the MAC 16 times), broadcast on ports 9 and 7.
pub async fn wake(mac: &str) -> AppResult<()> {
    let bytes: Vec<u8> = mac.split([':', '-']).filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
    if bytes.len() != 6 {
        return Err(AppError::validation("The TV's network address (MAC) isn't known, so it can't be switched on remotely. Pair it again while it's on."));
    }
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&bytes);
    }
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.map_err(|e| AppError::validation(format!("Couldn't send the wake-up signal ({e}).")))?;
    sock.set_broadcast(true).map_err(|e| AppError::validation(format!("Couldn't send the wake-up signal ({e}).")))?;
    for port in [9u16, 7] {
        sock.send_to(&packet, ("255.255.255.255", port)).await.map_err(|e| AppError::validation(format!("Couldn't send the wake-up signal ({e}).")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_ports_unless_a_port_is_given() {
        assert_eq!(endpoints("192.168.1.20"), vec!["ws://192.168.1.20:3000", "wss://192.168.1.20:3001"]);
        assert_eq!(endpoints("127.0.0.1:4567"), vec!["ws://127.0.0.1:4567"]);
    }

    #[test]
    fn the_certificate_seen_at_pairing_is_the_only_one_accepted_later() {
        use rustls::client::danger::ServerCertVerifier;
        let cert = rustls::pki_types::CertificateDer::from(b"tv certificate".to_vec());
        let other = rustls::pki_types::CertificateDer::from(b"someone else".to_vec());
        let name = rustls::pki_types::ServerName::try_from("192.168.1.20").unwrap();
        let now = rustls::pki_types::UnixTime::now();
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        // First pairing: nothing pinned, the certificate is remembered.
        let seen = Arc::new(Mutex::new(None));
        let first = PinnedCert { pinned: None, seen: seen.clone(), provider: provider.clone() };
        assert!(first.verify_server_cert(&cert, &[], &name, &[], now).is_ok());
        let pin = seen.lock().unwrap().clone().unwrap();
        assert_eq!(pin, fingerprint(b"tv certificate"));
        // Later: the same certificate passes, another one is refused.
        let later = PinnedCert { pinned: Some(pin), seen: Arc::new(Mutex::new(None)), provider };
        assert!(later.verify_server_cert(&cert, &[], &name, &[], now).is_ok());
        assert!(later.verify_server_cert(&other, &[], &name, &[], now).is_err());
    }

    #[test]
    fn pointer_events_on_the_wire() {
        assert_eq!(PointerEvent::Move { dx: 5, dy: -3 }.wire(), "type:move\ndx:5\ndy:-3\ndown:0\n\n");
        assert_eq!(PointerEvent::Click.wire(), "type:click\n\n");
        assert_eq!(PointerEvent::Scroll { dx: 0, dy: 40 }.wire(), "type:scroll\ndx:0\ndy:40\n\n");
        assert_eq!(PointerEvent::Move { dx: 99_999, dy: 0 }.wire(), "type:move\ndx:2000\ndy:0\ndown:0\n\n", "runaway values are clamped");
        let parsed: Vec<PointerEvent> = serde_json::from_str(r#"[{"type":"move","dx":3,"dy":4},{"type":"click"},{"type":"scroll","dx":0,"dy":-2}]"#).unwrap();
        assert_eq!(parsed, vec![PointerEvent::Move { dx: 3, dy: 4 }, PointerEvent::Click, PointerEvent::Scroll { dx: 0, dy: -2 }]);
        assert!(serde_json::from_str::<PointerEvent>(r#"{"type":"button","name":"POWER"}"#).is_err(), "buttons don't come in through the pointer route");
    }

    #[tokio::test]
    async fn wake_needs_a_real_mac() {
        assert!(wake("not-a-mac").await.is_err());
    }
}
