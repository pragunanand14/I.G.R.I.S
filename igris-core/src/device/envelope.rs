//! The cross-device message envelope.
//!
//! Every message between two of the user's devices is a versioned
//! [`Envelope`]:
//!
//! * the payload (including its message type) is encrypted to the recipient:
//!   an ephemeral X25519 key agreement with the recipient's static key, HKDF-
//!   SHA256 bound to both device ids, both keys and the message id, then
//!   ChaCha20-Poly1305 with the header as associated data;
//! * the header and ciphertext are signed with the sender's Ed25519 key;
//! * the receiver checks version, addressing, freshness (timestamp, expiry,
//!   clock skew), the signature against the *trusted* key it holds for the
//!   sender, and that it hasn't seen the message id before.
//!
//! The relay only ever sees the header (who → whom, when) and ciphertext.
//! All primitives come from audited RustCrypto / dalek crates; nothing here is
//! a new construction beyond composing them (an ECIES-style sealed box).

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey as KxPublic};
use zeroize::Zeroizing;

use super::identity::{decode32, random_hex, verify_sig, Identity, PublicDevice, B64};
use crate::error::{AppError, AppResult};

pub const VERSION: u8 = 1;
/// Longest lifetime a sender may give a message.
pub const MAX_TTL_MS: i64 = 15 * 60 * 1000;
/// Accepted difference between the two devices' clocks.
pub const MAX_SKEW_MS: i64 = 5 * 60 * 1000;
/// Largest plaintext payload.
pub const MAX_PAYLOAD: usize = 128 * 1024;

const DOMAIN: &[u8] = b"igris-envelope-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    /// Random message id (also the replay-protection key).
    pub id: String,
    pub from: String,
    pub to: String,
    /// Sent at (Unix ms, sender's clock).
    pub ts: i64,
    /// Not valid after (Unix ms).
    pub exp: i64,
    /// Ephemeral X25519 public key (base64url).
    pub epk: String,
    pub nonce: String,
    /// Encrypted payload (base64url).
    pub ct: String,
    /// Ed25519 signature over the header and ciphertext (base64url).
    pub sig: String,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Length-prefixed concatenation, so field boundaries can't be shifted.
fn frame(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len() + 4).sum::<usize>() + DOMAIN.len());
    out.extend_from_slice(DOMAIN);
    for p in parts {
        out.extend_from_slice(&(p.len() as u32).to_be_bytes());
        out.extend_from_slice(p);
    }
    out
}

impl Envelope {
    fn header_bytes(&self) -> Vec<u8> {
        frame(&[
            &[self.v],
            self.id.as_bytes(),
            self.from.as_bytes(),
            self.to.as_bytes(),
            &self.ts.to_be_bytes(),
            &self.exp.to_be_bytes(),
            self.epk.as_bytes(),
            self.nonce.as_bytes(),
        ])
    }

    fn signed_bytes(&self) -> Vec<u8> {
        let mut b = self.header_bytes();
        b.extend_from_slice(self.ct.as_bytes());
        b
    }
}

fn derive_key(shared: &[u8; 32], id: &str, from: &str, to: &str, epk: &[u8; 32], rpk: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(id.as_bytes()), shared);
    let info = frame(&[from.as_bytes(), to.as_bytes(), epk, rpk]);
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(&info, key.as_mut()).expect("32 bytes is a valid HKDF-SHA256 output length");
    key
}

/// Encrypt and sign `payload` for `to`, valid for `ttl_ms`.
pub fn seal(me: &Identity, to: &PublicDevice, payload: &[u8], ttl_ms: i64) -> AppResult<Envelope> {
    if payload.len() > MAX_PAYLOAD {
        return Err(AppError::validation("That message is too large to send to another device."));
    }
    let rpk = to.kx_public()?;
    let eph = EphemeralSecret::random_from_rng(OsRng);
    let epk = KxPublic::from(&eph);
    let shared = eph.diffie_hellman(&rpk);
    if !shared.was_contributory() {
        return Err(AppError::validation("The other device's key is invalid."));
    }
    let id = random_hex(16);
    let key = derive_key(shared.as_bytes(), &id, &me.device_id, &to.device_id, epk.as_bytes(), rpk.as_bytes());
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ts = now_ms();
    let mut env = Envelope {
        v: VERSION,
        id,
        from: me.device_id.clone(),
        to: to.device_id.clone(),
        ts,
        exp: ts + ttl_ms.clamp(1_000, MAX_TTL_MS),
        epk: B64.encode(epk.as_bytes()),
        nonce: B64.encode(nonce),
        ct: String::new(),
        sig: String::new(),
    };
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let ct =
        cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: payload, aad: &env.header_bytes() }).map_err(|_| AppError::internal("Encryption failed."))?;
    env.ct = B64.encode(ct);
    env.sig = B64.encode(me.sign(&env.signed_bytes()));
    Ok(env)
}

/// Why an envelope was refused. Shown in the device audit, never to the sender.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    #[error("unsupported envelope version")]
    Version,
    #[error("addressed to another device")]
    WrongRecipient,
    #[error("sender doesn't match the trusted key")]
    WrongSender,
    #[error("expired")]
    Expired,
    #[error("timestamp outside the accepted window")]
    Clock,
    #[error("bad signature")]
    Signature,
    #[error("already received (replay)")]
    Replay,
    #[error("can't decrypt")]
    Decrypt,
    #[error("malformed")]
    Malformed,
}

/// Check and decrypt an envelope from `sender` (a device this one already
/// trusts — the caller looks it up by `env.from`). Records the message id so
/// the same envelope is never accepted twice.
pub fn open(me: &Identity, sender: &PublicDevice, env: &Envelope, conn: &Connection) -> Result<Vec<u8>, Rejected> {
    open_at(me, sender, env, conn, now_ms())
}

pub fn open_at(me: &Identity, sender: &PublicDevice, env: &Envelope, conn: &Connection, now: i64) -> Result<Vec<u8>, Rejected> {
    if env.v != VERSION {
        return Err(Rejected::Version);
    }
    if env.to != me.device_id {
        return Err(Rejected::WrongRecipient);
    }
    if env.from != sender.device_id {
        return Err(Rejected::WrongSender);
    }
    if env.id.len() != 32 || !env.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Rejected::Malformed);
    }
    if env.exp <= now {
        return Err(Rejected::Expired);
    }
    if env.ts > now + MAX_SKEW_MS || env.exp - env.ts > MAX_TTL_MS || env.exp < env.ts {
        return Err(Rejected::Clock);
    }
    // Signature before anything else is trusted (including the replay table).
    verify_sig(sender, &env.signed_bytes(), &env.sig).map_err(|_| Rejected::Signature)?;

    // Replay: the id must be new. Kept until it could no longer be accepted anyway.
    let _ = conn.execute("DELETE FROM device_seen_messages WHERE expires_at < ?1", [now]);
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO device_seen_messages (msg_id, sender, expires_at) VALUES (?1, ?2, ?3)",
            params![env.id, env.from, env.exp + MAX_SKEW_MS],
        )
        .map_err(|_| Rejected::Malformed)?;
    if inserted == 0 {
        return Err(Rejected::Replay);
    }

    let epk = decode32(&env.epk).map_err(|_| Rejected::Malformed)?;
    let nonce: [u8; 12] = B64.decode(&env.nonce).ok().and_then(|n| n.try_into().ok()).ok_or(Rejected::Malformed)?;
    let ct = B64.decode(&env.ct).map_err(|_| Rejected::Malformed)?;
    let shared = me.kx_secret().diffie_hellman(&KxPublic::from(epk));
    if !shared.was_contributory() {
        return Err(Rejected::Decrypt);
    }
    let my_pk = KxPublic::from(me.kx_secret());
    let key = derive_key(shared.as_bytes(), &env.id, &env.from, &env.to, &epk, my_pk.as_bytes());
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: &env.header_bytes() }).map_err(|_| Rejected::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::device::identity::Platform;

    fn pair() -> (Identity, Identity, Database) {
        (Identity::generate("PC", Platform::Windows), Identity::generate("Phone", Platform::Android), Database::open_in_memory().unwrap())
    }

    #[test]
    fn round_trip_and_the_relay_sees_no_plaintext() {
        let (pc, phone, db) = pair();
        let env = seal(&phone, &pc.public(), b"open notepad on my pc", 60_000).unwrap();
        let wire = serde_json::to_string(&env).unwrap();
        assert!(!wire.contains("notepad"));
        let back = open(&pc, &phone.public(), &env, &db.conn().unwrap()).unwrap();
        assert_eq!(back, b"open notepad on my pc");
    }

    #[test]
    fn replays_are_refused() {
        let (pc, phone, db) = pair();
        let env = seal(&phone, &pc.public(), b"x", 60_000).unwrap();
        let conn = db.conn().unwrap();
        open(&pc, &phone.public(), &env, &conn).unwrap();
        assert_eq!(open(&pc, &phone.public(), &env, &conn), Err(Rejected::Replay));
    }

    #[test]
    fn tampering_anywhere_is_detected() {
        let (pc, phone, db) = pair();
        let conn = db.conn().unwrap();
        let env = seal(&phone, &pc.public(), b"hello", 60_000).unwrap();
        let mut ct = B64.decode(&env.ct).unwrap();
        ct[0] ^= 1;
        let tampered = [
            Envelope { ct: B64.encode(&ct), ..env.clone() },
            Envelope { exp: env.exp + 1, ..env.clone() },
            Envelope { ts: env.ts - 1, ..env.clone() },
            Envelope { nonce: B64.encode([0u8; 12]), ..env.clone() },
        ];
        for t in tampered {
            assert_eq!(open(&pc, &phone.public(), &t, &conn), Err(Rejected::Signature));
        }
        // The untouched one still opens (tampered copies weren't recorded as seen).
        assert!(open(&pc, &phone.public(), &env, &conn).is_ok());
    }

    #[test]
    fn only_the_trusted_sender_and_intended_recipient_are_accepted() {
        let (pc, phone, db) = pair();
        let conn = db.conn().unwrap();
        let mallory = Identity::generate("Mallory", Platform::Linux);
        // Mallory signs with her own key but claims to be the phone.
        let mut forged = seal(&mallory, &pc.public(), b"delete everything", 60_000).unwrap();
        forged.from = phone.device_id.clone();
        assert_eq!(open(&pc, &phone.public(), &forged, &conn), Err(Rejected::Signature));
        // Addressed to someone else.
        let other = seal(&phone, &mallory.public(), b"x", 60_000).unwrap();
        assert_eq!(open(&pc, &phone.public(), &other, &conn), Err(Rejected::WrongRecipient));
        // Re-addressing breaks the signature (and the key derivation).
        let mut moved = other.clone();
        moved.to = pc.device_id.clone();
        assert_eq!(open(&pc, &phone.public(), &moved, &conn), Err(Rejected::Signature));
        // Sender mismatch with the key the caller looked up.
        let env = seal(&phone, &pc.public(), b"x", 60_000).unwrap();
        assert_eq!(open(&pc, &mallory.public(), &env, &conn), Err(Rejected::WrongSender));
    }

    #[test]
    fn expired_and_future_messages_are_refused() {
        let (pc, phone, db) = pair();
        let conn = db.conn().unwrap();
        let env = seal(&phone, &pc.public(), b"x", 60_000).unwrap();
        assert_eq!(open_at(&pc, &phone.public(), &env, &conn, env.exp + 1), Err(Rejected::Expired));
        assert_eq!(open_at(&pc, &phone.public(), &env, &conn, env.ts - MAX_SKEW_MS - 1), Err(Rejected::Clock));
        // A TTL longer than allowed is clamped by the sender.
        let long = seal(&phone, &pc.public(), b"x", 24 * 3600 * 1000).unwrap();
        assert!(long.exp - long.ts <= MAX_TTL_MS);
    }

    #[test]
    fn oversized_payloads_are_refused() {
        let (pc, phone, _db) = pair();
        assert!(seal(&phone, &pc.public(), &vec![0u8; MAX_PAYLOAD + 1], 60_000).is_err());
    }
}
