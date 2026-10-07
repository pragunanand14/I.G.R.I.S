//! This installation's device identity.
//!
//! Each IGRIS installation has two long-lived key pairs: an Ed25519 key that
//! signs everything the device sends (and so *is* its identity — the device
//! id is a hash of its public signing key, never a hostname or address) and an
//! X25519 key that others encrypt to. The private halves never leave the
//! device: they are stored sealed by a platform [`KeyProtector`] (DPAPI on
//! Windows, the Android Keystore on Android).
//!
//! The owner id says which IGRIS user the device belongs to. A new device
//! starts with its own owner id and adopts its inviter's when it pairs.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as KxPublic, StaticSecret};
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

pub const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Seals private key material with an OS facility before it is stored.
pub trait KeyProtector: Send + Sync {
    /// Short name stored with the sealed keys ("dpapi", "android-keystore", "none").
    fn name(&self) -> &'static str;
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, String>;
    fn unprotect(&self, sealed: &[u8]) -> Result<Vec<u8>, String>;
}

/// No OS protection available (Linux/macOS builds, tests): the keys are kept as
/// is in the app's private database. Documented as weaker.
pub struct Unprotected;

impl KeyProtector for Unprotected {
    fn name(&self) -> &'static str {
        "none"
    }
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plain.to_vec())
    }
    fn unprotect(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
        Ok(sealed.to_vec())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Android,
    Linux,
    Macos,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Platform::Windows,
            "android" => Platform::Android,
            "linux" => Platform::Linux,
            "macos" => Platform::Macos,
            _ => Platform::Other,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Android => "android",
            Platform::Linux => "linux",
            Platform::Macos => "macos",
            Platform::Other => "other",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "windows" => Platform::Windows,
            "android" => Platform::Android,
            "linux" => Platform::Linux,
            "macos" => Platform::Macos,
            _ => Platform::Other,
        }
    }
    /// A phone (as opposed to a computer).
    pub fn is_phone(self) -> bool {
        matches!(self, Platform::Android)
    }
}

/// A device's public half, as shared with other devices and the relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDevice {
    pub device_id: String,
    pub name: String,
    pub platform: Platform,
    /// Ed25519 verifying key (base64url).
    pub signing_key: String,
    /// X25519 public key (base64url).
    pub kx_key: String,
}

impl PublicDevice {
    /// Check that the id really is derived from the signing key (so nobody can claim another device's id).
    pub fn verify_id(&self) -> AppResult<()> {
        let vk = decode32(&self.signing_key)?;
        if device_id_for(&vk) != self.device_id {
            return Err(AppError::validation("Device id doesn't match its key."));
        }
        decode32(&self.kx_key)?;
        Ok(())
    }

    pub fn verifying_key(&self) -> AppResult<VerifyingKey> {
        VerifyingKey::from_bytes(&decode32(&self.signing_key)?).map_err(|_| AppError::validation("Invalid device signing key."))
    }

    pub fn kx_public(&self) -> AppResult<KxPublic> {
        Ok(KxPublic::from(decode32(&self.kx_key)?))
    }
}

pub fn decode32(s: &str) -> AppResult<[u8; 32]> {
    let v = B64.decode(s).map_err(|_| AppError::validation("Invalid key encoding."))?;
    v.try_into().map_err(|_| AppError::validation("Invalid key length."))
}

/// `dev_` + the first 26 base32-ish hex chars of SHA-256(signing key).
pub fn device_id_for(signing_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(signing_key);
    format!("dev_{}", hex(&digest[..16]))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Random id (hex) of `n` bytes.
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    OsRng.fill_bytes(&mut b);
    hex(&b)
}

/// This device's identity, with its private keys (in memory only while needed).
pub struct Identity {
    pub device_id: String,
    pub owner_id: String,
    pub name: String,
    pub platform: Platform,
    signing: SigningKey,
    kx: StaticSecret,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("Identity").field("device_id", &self.device_id).field("owner_id", &self.owner_id).field("name", &self.name).finish()
    }
}

impl Identity {
    pub fn generate(name: &str, platform: Platform) -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let kx = StaticSecret::random_from_rng(OsRng);
        Self {
            device_id: device_id_for(signing.verifying_key().as_bytes()),
            owner_id: format!("own_{}", random_hex(16)),
            name: name.to_string(),
            platform,
            signing,
            kx,
        }
    }

    pub fn public(&self) -> PublicDevice {
        PublicDevice {
            device_id: self.device_id.clone(),
            name: self.name.clone(),
            platform: self.platform,
            signing_key: B64.encode(self.signing.verifying_key().as_bytes()),
            kx_key: B64.encode(KxPublic::from(&self.kx).as_bytes()),
        }
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }

    pub(crate) fn kx_secret(&self) -> &StaticSecret {
        &self.kx
    }

    /// Load this device's identity, creating (and storing) one on first run.
    pub fn load_or_create(conn: &Connection, protector: &dyn KeyProtector, default_name: &str, platform: Platform) -> AppResult<Self> {
        let row: Option<(String, String, String, String, Vec<u8>, String)> = conn
            .query_row("SELECT device_id, owner_id, name, platform, sealed_keys, protector FROM device_identity WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .optional()?;
        if let Some((device_id, owner_id, name, platform, sealed, used)) = row {
            if used != protector.name() {
                return Err(AppError::internal(format!("Device keys were sealed with {used}, which isn't available now.")));
            }
            let plain = Zeroizing::new(protector.unprotect(&sealed).map_err(|e| AppError::internal(format!("Couldn't unlock device keys: {e}")))?);
            if plain.len() != 64 {
                return Err(AppError::internal("Stored device keys are corrupt."));
            }
            let signing = SigningKey::from_bytes(plain[..32].try_into().expect("32 bytes"));
            let kx_bytes: [u8; 32] = plain[32..].try_into().expect("32 bytes");
            let kx = StaticSecret::from(kx_bytes);
            if device_id_for(signing.verifying_key().as_bytes()) != device_id {
                return Err(AppError::internal("Stored device keys don't match the device id."));
            }
            return Ok(Self { device_id, owner_id, name, platform: Platform::parse(&platform), signing, kx });
        }
        let id = Self::generate(default_name, platform);
        id.store(conn, protector)?;
        tracing::info!(event = "DEVICE_IDENTITY_CREATED", device_id = %id.device_id, protector = protector.name());
        Ok(id)
    }

    fn store(&self, conn: &Connection, protector: &dyn KeyProtector) -> AppResult<()> {
        let mut plain = Zeroizing::new(Vec::with_capacity(64));
        plain.extend_from_slice(&self.signing.to_bytes());
        plain.extend_from_slice(self.kx.as_bytes());
        let sealed = protector.protect(&plain).map_err(|e| AppError::internal(format!("Couldn't protect device keys: {e}")))?;
        conn.execute(
            "INSERT INTO device_identity (id, device_id, owner_id, name, platform, sealed_keys, protector) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET owner_id = excluded.owner_id, name = excluded.name",
            params![self.device_id, self.owner_id, self.name, self.platform.as_str(), sealed, protector.name()],
        )?;
        Ok(())
    }

    /// Join another device's IGRIS (after pairing).
    pub fn adopt_owner(&mut self, conn: &Connection, owner_id: &str) -> AppResult<()> {
        conn.execute("UPDATE device_identity SET owner_id = ?1 WHERE id = 1", params![owner_id])?;
        self.owner_id = owner_id.to_string();
        Ok(())
    }

    pub fn rename(&mut self, conn: &Connection, name: &str) -> AppResult<()> {
        let name = clean_name(name)?;
        conn.execute("UPDATE device_identity SET name = ?1 WHERE id = 1", params![name])?;
        self.name = name;
        Ok(())
    }
}

/// A user-visible device name: one line, 1–40 characters.
pub fn clean_name(name: &str) -> AppResult<String> {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if n.is_empty() || n.chars().count() > 40 {
        return Err(AppError::validation("A device name needs 1–40 characters."));
    }
    Ok(n)
}

pub fn verify_sig(device: &PublicDevice, msg: &[u8], sig: &str) -> AppResult<()> {
    let sig = B64.decode(sig).map_err(|_| AppError::validation("Invalid signature encoding."))?;
    let sig: [u8; 64] = sig.try_into().map_err(|_| AppError::validation("Invalid signature length."))?;
    device.verifying_key()?.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(&sig)).map_err(|_| AppError::validation("Signature doesn't verify."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    struct Xor;
    impl KeyProtector for Xor {
        fn name(&self) -> &'static str {
            "xor-test"
        }
        fn protect(&self, p: &[u8]) -> Result<Vec<u8>, String> {
            Ok(p.iter().map(|b| b ^ 0x5a).collect())
        }
        fn unprotect(&self, s: &[u8]) -> Result<Vec<u8>, String> {
            Ok(s.iter().map(|b| b ^ 0x5a).collect())
        }
    }

    #[test]
    fn identities_are_unique_derived_from_keys_and_persist() {
        let a = Identity::generate("A", Platform::Windows);
        let b = Identity::generate("B", Platform::Android);
        assert_ne!(a.device_id, b.device_id);
        assert_ne!(a.owner_id, b.owner_id);
        assert!(a.device_id.starts_with("dev_") && a.device_id.len() == 36);
        a.public().verify_id().unwrap();

        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let first = Identity::load_or_create(&conn, &Xor, "My PC", Platform::Windows).unwrap();
        let again = Identity::load_or_create(&conn, &Xor, "ignored", Platform::Windows).unwrap();
        assert_eq!(first.device_id, again.device_id);
        assert_eq!(first.public(), again.public(), "the same keys come back");
        // Sealed on disk, not stored in the clear.
        let sealed: Vec<u8> = conn.query_row("SELECT sealed_keys FROM device_identity", [], |r| r.get(0)).unwrap();
        assert!(!sealed.windows(32).any(|w| w == first.signing.to_bytes()));
        // A different protector can't open them.
        assert!(Identity::load_or_create(&conn, &Unprotected, "x", Platform::Windows).is_err());
    }

    #[test]
    fn a_claimed_id_must_match_the_key() {
        let a = Identity::generate("A", Platform::Windows);
        let mut p = a.public();
        p.device_id = Identity::generate("B", Platform::Windows).device_id;
        assert!(p.verify_id().is_err());
    }

    #[test]
    fn signatures_verify_only_for_the_signed_bytes_and_key() {
        let a = Identity::generate("A", Platform::Windows);
        let b = Identity::generate("B", Platform::Windows);
        let sig = B64.encode(a.sign(b"hello"));
        verify_sig(&a.public(), b"hello", &sig).unwrap();
        assert!(verify_sig(&a.public(), b"hellp", &sig).is_err());
        assert!(verify_sig(&b.public(), b"hello", &sig).is_err());
    }
}
