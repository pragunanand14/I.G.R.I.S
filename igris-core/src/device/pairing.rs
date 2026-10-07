//! Pairing a new device with explicit user confirmation.
//!
//! 1. On a device that is already set up (usually the PC), the user chooses
//!    "Pair a device". It shows a one-time code: a pairing id and an 80-bit
//!    secret, valid for 10 minutes and for at most 5 attempts.
//! 2. The user types the code on the new device. It sends its public keys to
//!    the inviter through the relay, encrypted and authenticated with a key
//!    derived from the secret (the relay only sees the pairing id).
//! 3. The inviter decrypts the request and asks its user: "Allow Pixel 8
//!    (Android) to join your IGRIS?". Nothing is trusted before that answer.
//! 4. If allowed, the inviter replies (again under the secret) with its own
//!    public keys, the owner id and the user's other devices. The new device
//!    adopts the owner id and trusts those keys.
//!
//! Being on the same network means nothing here; only the code and the
//! user's confirmation create trust. Each message is also signed by the
//! sender's long-term key, binding the keys exchanged to the code.

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::capability::Capability;
use super::envelope::now_ms;
use super::identity::{hex, verify_sig, Identity, PublicDevice, B64};
use crate::error::{AppError, AppResult};

pub const CODE_TTL_MS: i64 = 10 * 60 * 1000;
pub const MAX_ATTEMPTS: u32 = 5;
const ID_LEN: usize = 5;
const SECRET_LEN: usize = 10;

/// Crockford base32 (no I, L, O, U), read back case-insensitively.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn encode_code(bytes: &[u8]) -> String {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = String::new();
    for &b in bytes {
        bits = (bits << 8) | b as u32;
        n += 8;
        while n >= 5 {
            n -= 5;
            out.push(ALPHABET[((bits >> n) & 31) as usize] as char);
        }
    }
    if n > 0 {
        out.push(ALPHABET[((bits << (5 - n)) & 31) as usize] as char);
    }
    out.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect::<Vec<_>>().join("-")
}

fn decode_code(code: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in code.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = ALPHABET.iter().position(|&a| a as char == c)? as u32;
        bits = (bits << 5) | v;
        n += 5;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    Some(out)
}

/// A pairing code shown on the inviting device.
pub struct Invite {
    pub pairing_id: String,
    secret: Zeroizing<[u8; SECRET_LEN]>,
    pub expires_at: i64,
    attempts: u32,
}

impl std::fmt::Debug for Invite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invite").field("pairing_id", &self.pairing_id).field("expires_at", &self.expires_at).finish()
    }
}

impl Invite {
    /// A fresh invite and the code to show the user.
    pub fn new() -> (Invite, String) {
        let mut raw = Zeroizing::new([0u8; ID_LEN + SECRET_LEN]);
        OsRng.fill_bytes(raw.as_mut());
        let mut secret = Zeroizing::new([0u8; SECRET_LEN]);
        secret.copy_from_slice(&raw[ID_LEN..]);
        let code = encode_code(raw.as_ref());
        (Invite { pairing_id: hex(&raw[..ID_LEN]), secret, expires_at: now_ms() + CODE_TTL_MS, attempts: 0 }, code)
    }

    pub fn expired(&self) -> bool {
        self.expires_at <= now_ms() || self.attempts >= MAX_ATTEMPTS
    }

    /// Open a join request. Every failed attempt counts; after
    /// [`MAX_ATTEMPTS`] the code is dead.
    pub fn open_join(&mut self, blob: &str, relay_sender: &str) -> AppResult<JoinRequest> {
        if self.expired() {
            return Err(AppError::validation("This pairing code has expired. Start pairing again."));
        }
        self.attempts += 1;
        let req: JoinRequest = open_blob(&self.secret, &self.pairing_id, "join", blob)?;
        req.device.verify_id()?;
        // The relay authenticated the sender; it must be the device in the request.
        if req.device.device_id != relay_sender {
            return Err(AppError::validation("Pairing request didn't come from the device it describes."));
        }
        verify_sig(&req.device, &req.signed_bytes(&self.pairing_id), &req.sig)?;
        if (req.ts - now_ms()).abs() > CODE_TTL_MS {
            return Err(AppError::validation("Pairing request is stale."));
        }
        Ok(req)
    }

    /// The inviter's answer once its user allowed the new device.
    pub fn accept(&self, me: &Identity, my_caps: &[Capability], join: &JoinRequest, devices: Vec<KnownDevice>) -> AppResult<String> {
        let mut acc = JoinAccept {
            inviter: me.public(),
            inviter_caps: my_caps.to_vec(),
            owner_id: me.owner_id.clone(),
            joiner: join.device.device_id.clone(),
            devices,
            ts: now_ms(),
            sig: String::new(),
        };
        acc.sig = B64.encode(me.sign(&acc.signed_bytes(&self.pairing_id)));
        seal_blob(&self.secret, &self.pairing_id, "accept", &acc)
    }
}

/// A device the inviter already trusts, passed on to the new device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    pub device: PublicDevice,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub device: PublicDevice,
    pub capabilities: Vec<Capability>,
    pub ts: i64,
    pub sig: String,
}

impl JoinRequest {
    fn signed_bytes(&self, pairing_id: &str) -> Vec<u8> {
        let body = serde_json::json!({"pid": pairing_id, "device": self.device, "caps": self.capabilities, "ts": self.ts});
        [b"igris-pair-join-v1".as_slice(), body.to_string().as_bytes()].concat()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinAccept {
    pub inviter: PublicDevice,
    pub inviter_caps: Vec<Capability>,
    pub owner_id: String,
    /// The device this answer is for.
    pub joiner: String,
    pub devices: Vec<KnownDevice>,
    pub ts: i64,
    pub sig: String,
}

impl JoinAccept {
    fn signed_bytes(&self, pairing_id: &str) -> Vec<u8> {
        let body = serde_json::json!({
            "pid": pairing_id, "inviter": self.inviter, "caps": self.inviter_caps, "owner": self.owner_id,
            "joiner": self.joiner, "devices": self.devices, "ts": self.ts,
        });
        [b"igris-pair-accept-v1".as_slice(), body.to_string().as_bytes()].concat()
    }
}

/// A code typed on the new device.
pub struct Joining {
    pub pairing_id: String,
    secret: Zeroizing<[u8; SECRET_LEN]>,
}

impl Joining {
    pub fn from_code(code: &str) -> AppResult<Joining> {
        let raw = Zeroizing::new(decode_code(code).ok_or_else(|| AppError::validation("That code has characters that can't be in a pairing code."))?);
        if raw.len() != ID_LEN + SECRET_LEN {
            return Err(AppError::validation("A pairing code has 24 characters (like ABCD-EFGH-…). Check it and try again."));
        }
        let mut secret = Zeroizing::new([0u8; SECRET_LEN]);
        secret.copy_from_slice(&raw[ID_LEN..]);
        Ok(Joining { pairing_id: hex(&raw[..ID_LEN]), secret })
    }

    pub fn request(&self, me: &Identity, caps: &[Capability]) -> AppResult<String> {
        let mut req = JoinRequest { device: me.public(), capabilities: caps.to_vec(), ts: now_ms(), sig: String::new() };
        req.sig = B64.encode(me.sign(&req.signed_bytes(&self.pairing_id)));
        seal_blob(&self.secret, &self.pairing_id, "join", &req)
    }

    /// Open the inviter's answer: it must be for this device, signed by the inviter's key.
    pub fn open_accept(&self, me: &Identity, blob: &str, relay_sender: &str) -> AppResult<JoinAccept> {
        let acc: JoinAccept = open_blob(&self.secret, &self.pairing_id, "accept", blob)?;
        acc.inviter.verify_id()?;
        if acc.inviter.device_id != relay_sender {
            return Err(AppError::validation("Pairing answer didn't come from the inviting device."));
        }
        if acc.joiner != me.device_id {
            return Err(AppError::validation("Pairing answer was for another device."));
        }
        if !acc.owner_id.starts_with("own_") || acc.owner_id.len() != 36 {
            return Err(AppError::validation("Pairing answer is malformed."));
        }
        verify_sig(&acc.inviter, &acc.signed_bytes(&self.pairing_id), &acc.sig)?;
        for d in &acc.devices {
            d.device.verify_id()?;
        }
        Ok(acc)
    }
}

fn key(secret: &[u8; SECRET_LEN], pairing_id: &str, purpose: &str) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(pairing_id.as_bytes()), secret);
    let mut k = Zeroizing::new([0u8; 32]);
    hk.expand(format!("igris-pair-v1 {purpose}").as_bytes(), k.as_mut()).expect("valid length");
    k
}

fn seal_blob<T: Serialize>(secret: &[u8; SECRET_LEN], pairing_id: &str, purpose: &str, value: &T) -> AppResult<String> {
    let k = key(secret, pairing_id, purpose);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let plain = Zeroizing::new(serde_json::to_vec(value)?);
    let ct = ChaCha20Poly1305::new(Key::from_slice(k.as_ref()))
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: &plain, aad: pairing_id.as_bytes() })
        .map_err(|_| AppError::internal("Encryption failed."))?;
    Ok(B64.encode([nonce.as_slice(), &ct].concat()))
}

fn open_blob<T: for<'de> Deserialize<'de>>(secret: &[u8; SECRET_LEN], pairing_id: &str, purpose: &str, blob: &str) -> AppResult<T> {
    let wrong = || AppError::validation("The pairing code didn't match. Check it and try again.");
    let raw = B64.decode(blob).map_err(|_| wrong())?;
    if raw.len() < 12 + 16 || raw.len() > 64 * 1024 {
        return Err(wrong());
    }
    let k = key(secret, pairing_id, purpose);
    let plain = ChaCha20Poly1305::new(Key::from_slice(k.as_ref()))
        .decrypt(Nonce::from_slice(&raw[..12]), Payload { msg: &raw[12..], aad: pairing_id.as_bytes() })
        .map_err(|_| wrong())?;
    serde_json::from_slice(&plain).map_err(|_| wrong())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::identity::Platform;

    #[test]
    fn codes_round_trip_and_tolerate_typing() {
        let (invite, code) = Invite::new();
        assert_eq!(code.len(), 24 + 5, "{code}");
        let j = Joining::from_code(&code.to_lowercase().replace('-', " ")).unwrap();
        assert_eq!(j.pairing_id, invite.pairing_id);
        assert!(Joining::from_code("ABCD-EFGH").is_err());
        assert!(Joining::from_code("ABCD-EFGH-JKMN-PQRS-TVWX-YZ0U").is_err(), "U isn't in the alphabet");
        // Two invites never share a code.
        assert_ne!(Invite::new().1, code);
    }

    #[test]
    fn full_exchange() {
        let pc = Identity::generate("My PC", Platform::Windows);
        let phone = Identity::generate("Pixel", Platform::Android);
        let laptop = Identity::generate("Laptop", Platform::Windows);
        let (mut invite, code) = Invite::new();
        let joining = Joining::from_code(&code).unwrap();
        let blob = joining.request(&phone, &[Capability::Tasks, Capability::PhoneApps]).unwrap();
        assert!(!blob.contains("Pixel"), "the relay can't read the request");

        let req = invite.open_join(&blob, &phone.device_id).unwrap();
        assert_eq!(req.device, phone.public());
        let known = vec![KnownDevice { device: laptop.public(), capabilities: vec![Capability::Tasks] }];
        let reply = invite.accept(&pc, &[Capability::Tasks], &req, known).unwrap();
        let acc = joining.open_accept(&phone, &reply, &pc.device_id).unwrap();
        assert_eq!(acc.inviter, pc.public());
        assert_eq!(acc.owner_id, pc.owner_id);
        assert_eq!(acc.devices[0].device, laptop.public());
    }

    #[test]
    fn a_wrong_code_fails_and_attempts_are_limited() {
        let phone = Identity::generate("Pixel", Platform::Android);
        let (mut invite, _code) = Invite::new();
        let (_, other_code) = Invite::new();
        // Same pairing id, wrong secret.
        let mut wrong = Joining::from_code(&other_code).unwrap();
        wrong.pairing_id = invite.pairing_id.clone();
        let blob = wrong.request(&phone, &[]).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            assert!(invite.open_join(&blob, &phone.device_id).is_err());
        }
        assert!(invite.expired());
    }

    #[test]
    fn the_relay_cant_substitute_devices() {
        let pc = Identity::generate("My PC", Platform::Windows);
        let phone = Identity::generate("Pixel", Platform::Android);
        let mallory = Identity::generate("Mallory", Platform::Linux);
        let (mut invite, code) = Invite::new();
        let joining = Joining::from_code(&code).unwrap();
        let blob = joining.request(&phone, &[]).unwrap();
        // Relay claims the request came from another device.
        assert!(invite.open_join(&blob, &mallory.device_id).is_err());
        let req = invite.open_join(&blob, &phone.device_id).unwrap();
        let reply = invite.accept(&pc, &[], &req, vec![]).unwrap();
        // The answer is bound to the phone: another device can't use it…
        assert!(joining.open_accept(&mallory, &reply, &pc.device_id).is_err());
        // …and it must come from the inviter.
        assert!(joining.open_accept(&phone, &reply, &mallory.device_id).is_err());
    }
}
