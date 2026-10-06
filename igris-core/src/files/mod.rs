//! Filesystem access boundary.
//!
//! File tools can only touch paths inside folders the user explicitly allowed
//! (`allowed_folders`). Every path is resolved to its canonical form (following
//! symlinks) before the check, so `..` segments and links that point outside an
//! allowed folder are rejected. Writes additionally require the folder to be
//! marked writable. Known secret files are never readable.

use std::path::{Component, Path, PathBuf};

use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AllowedFolder {
    pub id: i64,
    pub path: String,
    pub writable: bool,
    pub created_at: String,
}

fn row(r: &Row) -> rusqlite::Result<AllowedFolder> {
    Ok(AllowedFolder { id: r.get(0)?, path: r.get(1)?, writable: r.get::<_, i64>(2)? != 0, created_at: r.get(3)? })
}

pub fn list(conn: &Connection) -> AppResult<Vec<AllowedFolder>> {
    let mut stmt = conn.prepare("SELECT id, path, writable, created_at FROM allowed_folders ORDER BY path")?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Folders that must never be granted wholesale (system roots, the home folder itself).
fn too_broad(p: &Path) -> bool {
    if p.parent().is_none() {
        return true; // "/" or "C:\\"
    }
    let s = p.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
    let s = s.trim_end_matches('/');
    // Whole trees that are system-only.
    let system_trees = ["/etc", "/proc", "/sys", "/dev", "/boot", "/bin", "/sbin", "/usr", "/system"];
    // Folders too broad to grant themselves (their subfolders are fine).
    let broad = ["/var", "/library", "/home", "/users", "/opt", "/mnt", "/media", "/tmp", "/private", "/applications"];
    let win = s.get(1..).unwrap_or_default(); // drop the drive letter
    system_trees.iter().any(|t| s == *t || s.starts_with(&format!("{t}/")))
        || broad.contains(&s)
        || [":/windows", ":/program files", ":/program files (x86)", ":/programdata"].iter().any(|t| win == *t || win.starts_with(&format!("{t}/")))
        || win == ":/users"
}

pub fn add(conn: &Connection, path: &str, writable: bool) -> AppResult<AllowedFolder> {
    let raw = PathBuf::from(path.trim());
    if !raw.is_absolute() {
        return Err(AppError::validation("Choose an absolute folder path."));
    }
    let canon = raw.canonicalize().map_err(|_| AppError::validation("That folder doesn't exist."))?;
    if !canon.is_dir() {
        return Err(AppError::validation("That path isn't a folder."));
    }
    if too_broad(&canon) {
        return Err(AppError::validation(
            "That folder is too broad to grant (a drive root or system folder). Choose a more specific folder, e.g. your user folder.",
        ));
    }
    if canon.file_name().is_some_and(is_secret_dir_name) {
        return Err(AppError::validation("That folder holds credentials or app data. IGRIS doesn't share those."));
    }
    let s = canon.display().to_string();
    match conn.execute("INSERT INTO allowed_folders (path, writable) VALUES (?1, ?2)", params![s, writable as i64]) {
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            return Err(AppError::validation("That folder is already allowed."));
        }
        r => r?,
    };
    tracing::info!(event = "FOLDER_ALLOWED", writable);
    Ok(conn.query_row("SELECT id, path, writable, created_at FROM allowed_folders WHERE id = ?1", [conn.last_insert_rowid()], row)?)
}

pub fn set_writable(conn: &Connection, id: i64, writable: bool) -> AppResult<()> {
    if conn.execute("UPDATE allowed_folders SET writable = ?2 WHERE id = ?1", params![id, writable as i64])? == 0 {
        return Err(AppError::validation("That folder is no longer allowed."));
    }
    Ok(())
}

pub fn remove(conn: &Connection, id: i64) -> AppResult<()> {
    if conn.execute("DELETE FROM allowed_folders WHERE id = ?1", [id])? == 0 {
        return Err(AppError::validation("That folder is no longer allowed."));
    }
    tracing::info!(event = "FOLDER_REVOKED");
    Ok(())
}

// Credential stores and app-data folders (browser profiles hold saved passwords
// and cookies). Matters most when the whole home folder is shared.
const SECRET_DIRS: &[&str] =
    &[".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker", ".password-store", "appdata", ".config", ".local", ".mozilla", ".thunderbird", "keychains"];

fn is_secret_dir_name(name: &std::ffi::OsStr) -> bool {
    SECRET_DIRS.contains(&name.to_string_lossy().to_ascii_lowercase().as_str())
}

/// Whether `path` (inside the shared folder `root`) is never read or written by tools:
/// credential files anywhere, and anything in a secret folder below the shared folder.
/// Only the part below `root` is checked for secret folders, so a folder the user
/// deliberately shared (which may itself live under e.g. AppData) still works.
pub fn is_secret(root: &Path, path: &Path) -> bool {
    let below = path.strip_prefix(root).unwrap_or(path);
    below.components().any(|c| matches!(c, Component::Normal(n) if is_secret_dir_name(n))) || is_secret_file(path)
}

/// File names that are never read or written by tools, even inside allowed folders.
pub fn is_secret_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    name == ".env"
        || name.starts_with(".env.")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || [".git-credentials", ".npmrc", ".pypirc", ".netrc", "credentials", "credentials.json", "secrets.json", "keychain"].contains(&name.as_str())
        || ["pem", "key", "p12", "pfx", "keystore", "jks", "kdbx", "asc", "gpg"].contains(&ext.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// A path that has passed the boundary check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedPath {
    /// Canonical absolute path (for new files: canonical parent + file name).
    pub path: PathBuf,
    pub root: PathBuf,
}

/// Resolve `requested` and confirm it lies inside an allowed folder with the
/// required access. Relative paths are resolved against the first allowed folder
/// only when exactly one folder is allowed (unambiguous).
pub fn guard(roots: &[AllowedFolder], requested: &str, access: Access) -> Result<GuardedPath, String> {
    if roots.is_empty() {
        return Err("No folders are shared with IGRIS yet. The user can add folders on the Tools page.".into());
    }
    let requested = requested.trim();
    if requested.is_empty() || requested.chars().any(char::is_control) {
        return Err("Invalid path.".into());
    }
    let mut p = PathBuf::from(requested);
    if !p.is_absolute() {
        if roots.len() != 1 {
            return Err("Use a full path inside one of the shared folders.".into());
        }
        p = PathBuf::from(&roots[0].path).join(p);
    }
    // Canonicalize the deepest existing ancestor, then append the rest
    // (which must not contain `..`).
    let mut existing = p.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return Err("Invalid path.".into()),
        }
    }
    let mut resolved = existing.canonicalize().map_err(|_| "Couldn't resolve that path.".to_string())?;
    for part in rest.iter().rev() {
        if part == ".." || part == "." {
            return Err("Paths may not contain '..'.".into());
        }
        resolved.push(part);
    }
    if Path::new(requested).components().any(|c| matches!(c, Component::ParentDir)) && !p.exists() {
        return Err("Paths may not contain '..'.".into());
    }

    let root = roots
        .iter()
        .filter_map(|r| PathBuf::from(&r.path).canonicalize().ok().map(|c| (c, r.writable)))
        .filter(|(c, _)| resolved.starts_with(c))
        .max_by_key(|(c, _)| c.components().count());
    let Some((root, writable)) = root else {
        return Err(format!("{} is outside the folders shared with IGRIS.", resolved.display()));
    };
    if access == Access::Write && !writable {
        return Err(format!("{} is shared read-only. The user can allow changes on the Tools page.", root.display()));
    }
    if is_secret(&root, &resolved) {
        return Err("That looks like a credentials or key file. IGRIS doesn't read or change those.".into());
    }
    Ok(GuardedPath { path: resolved, root })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn roots(dir: &Path, writable: bool) -> Vec<AllowedFolder> {
        vec![AllowedFolder { id: 1, path: dir.canonicalize().unwrap().display().to_string(), writable, created_at: String::new() }]
    }

    #[test]
    fn allows_paths_inside_and_rejects_escapes() {
        let base = tempfile::tempdir().unwrap();
        let shared = base.path().join("shared");
        std::fs::create_dir_all(shared.join("sub")).unwrap();
        std::fs::write(shared.join("sub/a.txt"), "hi").unwrap();
        std::fs::write(base.path().join("outside.txt"), "secret").unwrap();
        let r = roots(&shared, true);

        assert!(guard(&r, &shared.join("sub/a.txt").display().to_string(), Access::Read).is_ok());
        assert!(guard(&r, "sub/a.txt", Access::Read).is_ok(), "relative to the single root");
        assert!(guard(&r, &shared.join("new/file.txt").display().to_string(), Access::Write).is_ok(), "new paths resolve via existing ancestor");
        for bad in [
            base.path().join("outside.txt").display().to_string(),
            shared.join("../outside.txt").display().to_string(),
            shared.join("sub/../../outside.txt").display().to_string(),
            "../outside.txt".to_string(),
            shared.join("new/../../outside.txt").display().to_string(),
        ] {
            assert!(guard(&r, &bad, Access::Read).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape() {
        let base = tempfile::tempdir().unwrap();
        let shared = base.path().join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(base.path().join("outside.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(base.path().join("outside.txt"), shared.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(base.path(), shared.join("dirlink")).unwrap();
        let r = roots(&shared, true);
        assert!(guard(&r, &shared.join("link.txt").display().to_string(), Access::Read).is_err());
        assert!(guard(&r, &shared.join("dirlink/outside.txt").display().to_string(), Access::Read).is_err());
        assert!(guard(&r, &shared.join("dirlink/new.txt").display().to_string(), Access::Write).is_err());
    }

    #[test]
    fn read_only_folders_reject_writes_and_secrets_are_off_limits() {
        let base = tempfile::tempdir().unwrap();
        let r = roots(base.path(), false);
        assert!(guard(&r, "notes.txt", Access::Write).unwrap_err().contains("read-only"));
        for secret in [".env", ".env.local", "id_rsa", "server.pem", "cert.p12", ".ssh/config", "aws/credentials"] {
            let p = base.path().join(secret);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
            assert!(guard(&r, secret, Access::Read).unwrap_err().contains("credentials"), "{secret}");
        }
        assert!(guard(&[], "x", Access::Read).unwrap_err().contains("No folders"));
    }

    #[test]
    fn granting_folders_validates() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let f = add(&conn, &dir.path().display().to_string(), false).unwrap();
        assert!(!f.writable);
        assert!(add(&conn, &dir.path().display().to_string(), true).is_err(), "duplicates rejected");
        assert!(add(&conn, "relative/path", false).is_err());
        assert!(add(&conn, "/definitely/missing", false).is_err());
        // The drive/file-system root ("/" on Unix, e.g. "C:\\" on Windows).
        let root = dir.path().ancestors().last().unwrap().display().to_string();
        assert!(add(&conn, &root, false).unwrap_err().to_string().contains("too broad"), "{root}");
        assert!(add(&conn, "/etc", false).is_err());
        // The home folder itself may be shared; its app-data and credential folders stay off-limits.
        let home = Path::new("/home/u");
        assert!(is_secret(home, Path::new("/home/u/AppData/Local/Google/Chrome/User Data/Default/Login Data")));
        assert!(is_secret(home, Path::new("/home/u/.config/app/settings.json")));
        assert!(!is_secret(home, Path::new("/home/u/Documents/config-notes.txt")));
        // A folder deliberately shared from inside app data is usable; secret folders can't be shared.
        assert!(!is_secret(Path::new("/home/u/AppData/Local/Temp/x"), Path::new("/home/u/AppData/Local/Temp/x/a.txt")));
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        assert!(add(&conn, &ssh.display().to_string(), false).unwrap_err().to_string().contains("credentials"));
        set_writable(&conn, f.id, true).unwrap();
        assert!(list(&conn).unwrap()[0].writable);
        remove(&conn, f.id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }
}
