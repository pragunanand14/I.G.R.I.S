//! The trusted device registry: the user's other devices, by public key.
//!
//! A device gets here only through explicit pairing (or an announcement from a
//! device that is already trusted and allowed to manage devices). Revoking
//! keeps the row, marked revoked, so a revoked key can never quietly come back
//! through a stale announcement: only a fresh pairing confirmed by the user
//! re-trusts it.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::capability::{self, Capability};
use super::identity::{Platform, PublicDevice};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_id: String,
    pub owner_id: String,
    pub name: String,
    pub platform: Platform,
    #[serde(skip)]
    pub signing_key: String,
    #[serde(skip)]
    pub kx_key: String,
    pub capabilities: Vec<Capability>,
    pub trusted: bool,
    pub revoked_at: Option<String>,
    pub last_seen: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Device {
    pub fn public(&self) -> PublicDevice {
        PublicDevice {
            device_id: self.device_id.clone(),
            name: self.name.clone(),
            platform: self.platform,
            signing_key: self.signing_key.clone(),
            kx_key: self.kx_key.clone(),
        }
    }

    /// Trusted and not revoked.
    pub fn active(&self) -> bool {
        self.trusted && self.revoked_at.is_none()
    }
}

const COLS: &str = "device_id, owner_id, name, platform, signing_key, kx_key, capabilities, trusted, revoked_at, last_seen, created_at, updated_at";

fn row(r: &Row) -> rusqlite::Result<Device> {
    let platform: String = r.get(3)?;
    let caps: String = r.get(6)?;
    Ok(Device {
        device_id: r.get(0)?,
        owner_id: r.get(1)?,
        name: r.get(2)?,
        platform: Platform::parse(&platform),
        signing_key: r.get(4)?,
        kx_key: r.get(5)?,
        capabilities: capability::parse_list(&caps),
        trusted: r.get::<_, i64>(7)? != 0,
        revoked_at: r.get(8)?,
        last_seen: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

pub fn get(conn: &Connection, device_id: &str) -> AppResult<Option<Device>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM devices WHERE device_id = ?1"), [device_id], row).optional()?)
}

/// A device this one currently trusts, or `None`.
pub fn trusted(conn: &Connection, device_id: &str) -> AppResult<Option<Device>> {
    Ok(get(conn, device_id)?.filter(Device::active))
}

pub fn list(conn: &Connection) -> AppResult<Vec<Device>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM devices ORDER BY revoked_at IS NOT NULL, created_at"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn active(conn: &Connection) -> AppResult<Vec<Device>> {
    Ok(list(conn)?.into_iter().filter(Device::active).collect())
}

/// How a device came to be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustSource {
    /// The user confirmed a pairing on this device (may re-trust a revoked key).
    Pairing,
    /// A trusted device told us about it (never overrides a revocation).
    Announcement,
}

/// Trust `device` as one of `owner_id`'s devices.
pub fn trust(conn: &Connection, device: &PublicDevice, owner_id: &str, caps: &[Capability], source: TrustSource) -> AppResult<Device> {
    device.verify_id()?;
    if let Some(existing) = get(conn, &device.device_id)? {
        if existing.revoked_at.is_some() && source == TrustSource::Announcement {
            return Err(AppError::validation("That device was removed; it can only come back by pairing again."));
        }
        if existing.signing_key != device.signing_key {
            // Unreachable while ids are key hashes; kept as a hard stop.
            return Err(AppError::validation("A device with that id already exists with another key."));
        }
    }
    let name = super::identity::clean_name(&device.name).unwrap_or_else(|_| "Device".into());
    conn.execute(
        "INSERT INTO devices (device_id, owner_id, name, platform, signing_key, kx_key, capabilities, trusted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
         ON CONFLICT(device_id) DO UPDATE SET owner_id = ?2, name = ?3, platform = ?4, kx_key = ?6, capabilities = ?7,
             trusted = 1, revoked_at = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        params![
            device.device_id,
            owner_id,
            name,
            device.platform.as_str(),
            device.signing_key,
            device.kx_key,
            serde_json::to_string(caps).unwrap_or_else(|_| "[]".into())
        ],
    )?;
    get(conn, &device.device_id)?.ok_or_else(|| AppError::internal("device vanished"))
}

pub fn revoke(conn: &Connection, device_id: &str) -> AppResult<Device> {
    let n = conn.execute(
        "UPDATE devices SET trusted = 0, revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE device_id = ?1",
        [device_id],
    )?;
    if n == 0 {
        return Err(AppError::validation("That device isn't paired with this one."));
    }
    get(conn, device_id)?.ok_or_else(|| AppError::internal("device vanished"))
}

/// Forget a revoked device entirely (its key could then re-pair like a new one).
pub fn forget(conn: &Connection, device_id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM devices WHERE device_id = ?1 AND revoked_at IS NOT NULL", [device_id])?;
    if n == 0 {
        return Err(AppError::validation("Remove the device first."));
    }
    conn.execute("DELETE FROM sync_peers WHERE device_id = ?1", [device_id])?;
    Ok(())
}

pub fn rename(conn: &Connection, device_id: &str, name: &str) -> AppResult<Device> {
    let name = super::identity::clean_name(name)?;
    conn.execute("UPDATE devices SET name = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE device_id = ?1", params![device_id, name])?;
    get(conn, device_id)?.ok_or_else(|| AppError::validation("That device isn't paired with this one."))
}

pub fn set_capabilities(conn: &Connection, device_id: &str, caps: &[Capability]) -> AppResult<()> {
    conn.execute("UPDATE devices SET capabilities = ?2 WHERE device_id = ?1", params![device_id, serde_json::to_string(caps).unwrap_or_else(|_| "[]".into())])?;
    Ok(())
}

pub fn touch(conn: &Connection, device_id: &str) -> AppResult<()> {
    conn.execute("UPDATE devices SET last_seen = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE device_id = ?1", [device_id])?;
    Ok(())
}

/// Record something another device did (or tried) for the cross-device audit.
pub fn audit(conn: &Connection, peer: Option<&str>, kind: &str, detail: Option<&str>) {
    let detail = detail.map(|d| crate::orchestrator::task::clip(d, 300));
    if let Err(e) = conn.execute("INSERT INTO device_audit (peer, kind, detail) VALUES (?1, ?2, ?3)", params![peer, kind, detail]) {
        tracing::warn!(event = "DEVICE_AUDIT_WRITE_FAILED", error = %e);
    }
    // Bounded.
    let _ = conn.execute("DELETE FROM device_audit WHERE id <= (SELECT MAX(id) - 2000 FROM device_audit)", []);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: i64,
    pub at: String,
    pub peer: Option<String>,
    pub kind: String,
    pub detail: Option<String>,
}

pub fn audit_log(conn: &Connection, limit: u32) -> AppResult<Vec<AuditRow>> {
    let mut stmt = conn.prepare("SELECT id, at, peer, kind, detail FROM device_audit ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit.min(500)], |r| Ok(AuditRow { id: r.get(0)?, at: r.get(1)?, peer: r.get(2)?, kind: r.get(3)?, detail: r.get(4)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::device::identity::Identity;

    #[test]
    fn trust_revoke_and_no_silent_comeback() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let phone = Identity::generate("Pixel", Platform::Android);
        let d = trust(&conn, &phone.public(), "own_x", &[Capability::Tasks], TrustSource::Pairing).unwrap();
        assert!(d.active());
        assert_eq!(trusted(&conn, &phone.device_id).unwrap().unwrap().capabilities, vec![Capability::Tasks]);

        revoke(&conn, &phone.device_id).unwrap();
        assert!(trusted(&conn, &phone.device_id).unwrap().is_none());
        // An announcement from another device can't bring it back…
        assert!(trust(&conn, &phone.public(), "own_x", &[], TrustSource::Announcement).is_err());
        assert!(trusted(&conn, &phone.device_id).unwrap().is_none());
        // …only pairing again, confirmed by the user.
        trust(&conn, &phone.public(), "own_x", &[], TrustSource::Pairing).unwrap();
        assert!(trusted(&conn, &phone.device_id).unwrap().is_some());
    }

    #[test]
    fn a_device_whose_id_doesnt_match_its_key_is_refused() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let mut p = Identity::generate("A", Platform::Android).public();
        p.device_id = "dev_00000000000000000000000000000000".into();
        assert!(trust(&conn, &p, "own_x", &[], TrustSource::Pairing).is_err());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn keys_are_not_serialized_to_the_ui() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let phone = Identity::generate("Pixel", Platform::Android);
        let d = trust(&conn, &phone.public(), "own_x", &[], TrustSource::Pairing).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains(&phone.public().signing_key));
    }
}
