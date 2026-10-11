//! Tidying the Downloads folder: files go into folders by kind (Documents,
//! Images, Videos, Music, Archives, Installers). Only files directly in the
//! folder are moved; folders, hidden files, downloads still in progress and
//! files changed in the last few minutes stay. Nothing is renamed or
//! overwritten (a name already taken gets " (2)"), and the last tidy can be
//! undone.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Files this new are left alone (they may still be downloading or open).
pub const SETTLE: Duration = Duration::from_secs(180);

/// The settings key (extra) for "tidy by itself", and for the last tidy (undo).
pub const AUTO_KEY: &str = "downloadsAutoTidy";
pub const LAST_KEY: &str = "downloadsLastTidy";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// The folder a file name belongs in, or `None` to leave it.
pub fn folder_for(name: &str) -> Option<&'static str> {
    let ext = Path::new(name).extension()?.to_string_lossy().to_lowercase();
    Some(match ext.as_str() {
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "xls" | "xlsx" | "ods" | "csv" | "ppt" | "pptx" | "odp" | "epub" | "pages" | "numbers"
        | "key" => "Documents",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "bmp" | "svg" | "tif" | "tiff" | "avif" => "Images",
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "wmv" => "Videos",
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wma" => "Music",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" => "Archives",
        "exe" | "msi" | "msix" | "appx" | "apk" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => "Installers",
        _ => return None,
    })
}

fn in_progress(name: &str) -> bool {
    let n = name.to_lowercase();
    [".crdownload", ".part", ".partial", ".download", ".tmp", ".opdownload", ".!ut"].iter().any(|e| n.ends_with(e)) || n == "desktop.ini"
}

/// A free name for `name` in `dir`: the name itself, else "name (2).ext", …
fn free_name(dir: &Path, name: &str, taken: &[PathBuf]) -> PathBuf {
    let p = Path::new(name);
    let (stem, ext) = (p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), p.extension().map(|e| e.to_string_lossy().into_owned()));
    let mut candidate = dir.join(name);
    let mut n = 2;
    while candidate.exists() || taken.contains(&candidate) {
        candidate = dir.join(match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        });
        n += 1;
    }
    candidate
}

/// What tidying `dir` would move now.
pub fn plan(dir: &Path, now: SystemTime) -> std::io::Result<Vec<Move>> {
    let mut moves = Vec::new();
    let mut taken = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() || name.starts_with('.') || in_progress(&name) {
            continue;
        }
        if !meta.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age >= SETTLE) {
            continue;
        }
        let Some(folder) = folder_for(&name) else { continue };
        let to = free_name(&dir.join(folder), &name, &taken);
        taken.push(to.clone());
        moves.push(Move { from: e.path(), to });
    }
    Ok(moves)
}

/// Do the moves; returns those done and the errors (a file in use stays put).
pub fn apply(moves: &[Move]) -> (Vec<Move>, Vec<String>) {
    let (mut done, mut errors) = (Vec::new(), Vec::new());
    for m in moves {
        let r = m.to.parent().map(std::fs::create_dir_all).transpose().and_then(|_| {
            if m.to.exists() {
                Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "name taken"))
            } else {
                std::fs::rename(&m.from, &m.to)
            }
        });
        match r {
            Ok(()) => done.push(m.clone()),
            Err(e) => errors.push(format!("{}: {e}", m.from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        }
    }
    (done, errors)
}

/// Put files back where they were (skipping any moved or replaced since).
pub fn undo(moves: &[Move]) -> usize {
    moves.iter().filter(|m| m.to.is_file() && !m.from.exists() && std::fs::rename(&m.to, &m.from).is_ok()).count()
}

/// "12 files: 5 Documents, 4 Images, 3 Installers".
pub fn describe(moves: &[Move]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for m in moves {
        let f = m.to.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match counts.iter_mut().find(|(k, _)| *k == f) {
            Some(c) => c.1 += 1,
            None => counts.push((f, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let parts: Vec<String> = counts.iter().map(|(k, n)| format!("{n} {k}")).collect();
    format!("{} file{}: {}", moves.len(), if moves.len() == 1 { "" } else { "s" }, parts.join(", "))
}

/// Tidy `dir` now and remember what moved (for undo). Returns the moves done and any errors.
pub fn run(conn: &rusqlite::Connection, dir: &Path) -> crate::error::AppResult<(Vec<Move>, Vec<String>)> {
    let moves = plan(dir, SystemTime::now()).map_err(|e| crate::error::AppError::validation(format!("Couldn't read {}: {e}", dir.display())))?;
    let (done, errors) = apply(&moves);
    if !done.is_empty() {
        crate::settings::set_extra(conn, LAST_KEY, &done)?;
        tracing::info!(event = "DOWNLOADS_TIDIED", count = done.len());
    }
    Ok((done, errors))
}

/// Undo the last tidy. Returns how many files went back.
pub fn undo_last(conn: &rusqlite::Connection) -> crate::error::AppResult<usize> {
    let last: Vec<Move> = crate::settings::get_extra(conn, LAST_KEY)?.unwrap_or_default();
    let n = undo(&last);
    crate::settings::set_extra(conn, LAST_KEY, &Vec::<Move>::new())?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_settled_files_and_undoes() {
        let d = tempfile::tempdir().unwrap();
        let w = |n: &str| std::fs::write(d.path().join(n), b"x").unwrap();
        for n in ["report.pdf", "photo.JPG", "setup.exe", "notes", ".hidden.pdf", "movie.mp4.crdownload", "song.mp3"] {
            w(n);
        }
        std::fs::create_dir_all(d.path().join("Documents")).unwrap();
        std::fs::write(d.path().join("Documents/report.pdf"), b"old").unwrap();
        std::fs::create_dir(d.path().join("Some folder")).unwrap();

        // Just downloaded: nothing moves yet.
        assert!(plan(d.path(), SystemTime::now()).unwrap().is_empty());
        let later = SystemTime::now() + SETTLE + Duration::from_secs(1);
        let moves = plan(d.path(), later).unwrap();
        assert_eq!(describe(&moves), "4 files: 1 Documents, 1 Images, 1 Installers, 1 Music");
        assert!(moves.iter().any(|m| m.to.ends_with("Documents/report (2).pdf")), "nothing is overwritten");
        let (done, errors) = apply(&moves);
        assert_eq!((done.len(), errors.len()), (4, 0));
        assert!(d.path().join("Images/photo.JPG").is_file() && d.path().join("notes").is_file() && d.path().join("movie.mp4.crdownload").is_file());
        assert_eq!(std::fs::read(d.path().join("Documents/report.pdf")).unwrap(), b"old");

        assert_eq!(undo(&done), 4);
        assert!(d.path().join("report.pdf").is_file() && d.path().join("setup.exe").is_file());
    }
}
