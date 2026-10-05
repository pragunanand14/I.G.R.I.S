//! Images and PDFs attached to messages, and screenshots taken by tools.
//!
//! Files live in `<app data>/attachments/<id>.<ext>`; rows in the
//! `attachments` table. An upload is *staged* (no conversation) until a message
//! is sent with it. Rows cascade with their conversation/message, and
//! [`AttachmentStore::gc`] removes files whose row is gone plus stale staged
//! uploads. The bytes are read from disk on every request, so replayed history
//! is identical each time.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::ai::{Media, MediaKind};
use crate::error::{AppError, AppResult};

pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_PDF_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_IMAGE_SIDE: u32 = 8000;
pub const MAX_PER_MESSAGE: usize = 5;
/// Extracted PDF text sent to providers without native PDF input.
pub const MAX_PDF_TEXT_CHARS: usize = 100_000;
/// Staged uploads nobody sent are removed after this long.
const STAGED_TTL_HOURS: i64 = 24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub kind: MediaKind,
    pub mime: String,
    pub name: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// "upload" or "screenshot".
    pub source: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub created_at: String,
}

impl Attachment {
    pub fn media(&self) -> Media {
        Media { attachment_id: self.id.clone(), kind: self.kind, mime: self.mime.clone(), name: self.name.clone(), data: None, text: None }
    }
}

const COLS: &str = "id, kind, mime, name, size, width, height, source, conversation_id, message_id, created_at";

fn row(r: &Row) -> rusqlite::Result<Attachment> {
    let kind: String = r.get(1)?;
    Ok(Attachment {
        id: r.get(0)?,
        kind: if kind == "pdf" { MediaKind::Pdf } else { MediaKind::Image },
        mime: r.get(2)?,
        name: r.get(3)?,
        size: r.get(4)?,
        width: r.get(5)?,
        height: r.get(6)?,
        source: r.get(7)?,
        conversation_id: r.get(8)?,
        message_id: r.get(9)?,
        created_at: r.get(10)?,
    })
}

fn kind_str(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Image => "image",
        MediaKind::Pdf => "pdf",
    }
}

fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "pdf",
    }
}

/// Identify supported content by its bytes (never by the claimed name or type).
pub fn sniff(bytes: &[u8]) -> Option<(MediaKind, &'static str)> {
    if bytes.starts_with(b"%PDF-") {
        return Some((MediaKind::Pdf, "application/pdf"));
    }
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some((MediaKind::Image, "image/png")),
        image::ImageFormat::Jpeg => Some((MediaKind::Image, "image/jpeg")),
        image::ImageFormat::Gif => Some((MediaKind::Image, "image/gif")),
        image::ImageFormat::WebP => Some((MediaKind::Image, "image/webp")),
        _ => None,
    }
}

/// A display-safe file name: no path parts or control characters, bounded length.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let s: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let s = s.trim();
    if s.is_empty() {
        fallback.to_string()
    } else {
        s.to_string()
    }
}

fn image_dims(bytes: &[u8]) -> AppResult<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| AppError::validation("That image couldn't be read."))?
        .into_dimensions()
        .map_err(|_| AppError::validation("That image couldn't be read — it may be damaged."))
}

/// Text from a PDF, bounded; `None` when nothing is extractable (e.g. scanned pages).
pub fn extract_pdf_text(bytes: &[u8]) -> Option<String> {
    // The PDF parser can panic on malformed input; contain it.
    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes)).ok()?.ok()?;
    let text = text.split('\n').map(str::trim_end).collect::<Vec<_>>().join("\n");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut out: String = text.chars().take(MAX_PDF_TEXT_CHARS).collect();
    if text.chars().count() > MAX_PDF_TEXT_CHARS {
        out.push_str("\n[document text truncated]");
    }
    Some(out)
}

#[derive(Debug)]
pub struct AttachmentStore {
    dir: PathBuf,
}

impl AttachmentStore {
    pub fn new(dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn file(&self, a: &Attachment) -> PathBuf {
        self.dir.join(format!("{}.{}", a.id, ext_for(&a.mime)))
    }

    fn text_file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.txt"))
    }

    fn insert(&self, conn: &Connection, bytes: &[u8], (kind, mime): (MediaKind, &str), name: &str, dims: Option<(u32, u32)>, source: &str) -> AppResult<Attachment> {
        let id = uuid::Uuid::new_v4().to_string();
        let a = Attachment {
            id: id.clone(),
            kind,
            mime: mime.into(),
            name: name.into(),
            size: bytes.len() as i64,
            width: dims.map(|d| d.0 as i64),
            height: dims.map(|d| d.1 as i64),
            source: source.into(),
            conversation_id: None,
            message_id: None,
            created_at: String::new(),
        };
        std::fs::write(self.file(&a), bytes)?;
        let r = conn.execute(
            "INSERT INTO attachments (id, kind, mime, name, size, width, height, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![a.id, kind_str(kind), a.mime, a.name, a.size, a.width, a.height, a.source],
        );
        if let Err(e) = r {
            let _ = std::fs::remove_file(self.file(&a));
            return Err(e.into());
        }
        get(conn, &id)?.ok_or_else(|| AppError::internal("attachment vanished"))
    }

    /// Validate and stage an upload. The type comes from the bytes, not the name.
    pub fn save_upload(&self, conn: &Connection, name: &str, bytes: &[u8]) -> AppResult<Attachment> {
        let (kind, mime) = sniff(bytes).ok_or_else(|| AppError::validation("Only PNG, JPEG, GIF and WebP images and PDF files can be attached."))?;
        let name = clean_name(name, if kind == MediaKind::Pdf { "document.pdf" } else { "image" });
        match kind {
            MediaKind::Image => {
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(AppError::validation(format!("Images can be at most {} MB.", MAX_IMAGE_BYTES / 1024 / 1024)));
                }
                let (w, h) = image_dims(bytes)?;
                if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE {
                    return Err(AppError::validation(format!("Images can be at most {MAX_IMAGE_SIDE}×{MAX_IMAGE_SIDE} pixels.")));
                }
                self.insert(conn, bytes, (kind, mime), &name, Some((w, h)), "upload")
            }
            MediaKind::Pdf => {
                if bytes.len() > MAX_PDF_BYTES {
                    return Err(AppError::validation(format!("PDFs can be at most {} MB.", MAX_PDF_BYTES / 1024 / 1024)));
                }
                let a = self.insert(conn, bytes, (kind, mime), &name, None, "upload")?;
                if let Some(text) = extract_pdf_text(bytes) {
                    std::fs::write(self.text_file(&a.id), text)?;
                }
                Ok(a)
            }
        }
    }

    /// Store a tool-captured image (JPEG/PNG bytes); unlinked until the executor adopts it.
    pub fn save_capture(&self, conn: &Connection, name: &str, mime: &str, bytes: &[u8], dims: (u32, u32)) -> AppResult<Attachment> {
        self.insert(conn, bytes, (MediaKind::Image, mime), name, Some(dims), "screenshot")
    }

    pub fn read_base64(&self, a: &Attachment) -> AppResult<String> {
        Ok(base64::engine::general_purpose::STANDARD.encode(std::fs::read(self.file(a))?))
    }

    /// Fill in bytes (and PDF text) for a request. Missing files leave `data` empty.
    pub fn hydrate(&self, conn: &Connection, media: &mut Media) {
        let Ok(Some(a)) = get(conn, &media.attachment_id) else { return };
        match self.read_base64(&a) {
            Ok(b64) => media.data = Some(Arc::from(b64)),
            Err(e) => tracing::warn!(event = "ATTACHMENT_READ_FAILED", id = %a.id, error = %e),
        }
        if a.kind == MediaKind::Pdf {
            media.text = std::fs::read_to_string(self.text_file(&a.id)).ok().map(Arc::from);
        }
    }

    /// Delete a staged upload the user removed before sending.
    pub fn discard(&self, conn: &Connection, id: &str) -> AppResult<()> {
        let a = get(conn, id)?.ok_or_else(|| AppError::validation("That attachment no longer exists."))?;
        if a.message_id.is_some() || a.conversation_id.is_some() {
            return Err(AppError::validation("That attachment is part of a conversation."));
        }
        conn.execute("DELETE FROM attachments WHERE id = ?1", [id])?;
        self.remove_files(&a.id);
        Ok(())
    }

    fn remove_files(&self, id: &str) {
        for ext in ["png", "jpg", "gif", "webp", "pdf", "txt"] {
            let _ = std::fs::remove_file(self.dir.join(format!("{id}.{ext}")));
        }
    }

    /// Remove stale staged rows and files whose row is gone. Returns files removed.
    pub fn gc(&self, conn: &Connection) -> AppResult<usize> {
        conn.execute(
            "DELETE FROM attachments WHERE conversation_id IS NULL AND message_id IS NULL \
             AND created_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)",
            [format!("-{STAGED_TTL_HOURS} hours")],
        )?;
        let mut stmt = conn.prepare("SELECT 1 FROM attachments WHERE id = ?1")?;
        let mut removed = 0;
        for entry in std::fs::read_dir(&self.dir)?.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if uuid::Uuid::parse_str(stem).is_err() {
                continue;
            }
            if stmt.query_row([stem], |_| Ok(())).optional()?.is_none() && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Attachment>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM attachments WHERE id = ?1"), [id], row).optional()?)
}

/// Attach staged uploads to a newly saved user message.
pub fn link_to_message(conn: &Connection, ids: &[String], conversation_id: &str, message_id: &str) -> AppResult<Vec<Attachment>> {
    let mut out = Vec::new();
    for id in ids {
        let n = conn.execute(
            "UPDATE attachments SET conversation_id = ?1, message_id = ?2 \
             WHERE id = ?3 AND source = 'upload' AND conversation_id IS NULL AND message_id IS NULL",
            params![conversation_id, message_id, id],
        )?;
        if n == 0 {
            return Err(AppError::validation("An attachment is missing or was already sent — attach it again."));
        }
        out.extend(get(conn, id)?);
    }
    Ok(out)
}

/// Check staged uploads before anything is saved.
pub fn check_staged(conn: &Connection, ids: &[String]) -> AppResult<()> {
    if ids.len() > MAX_PER_MESSAGE {
        return Err(AppError::validation(format!("Attach at most {MAX_PER_MESSAGE} files per message.")));
    }
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(AppError::validation("The same attachment was added twice."));
        }
        match get(conn, id)? {
            Some(a) if a.source == "upload" && a.message_id.is_none() && a.conversation_id.is_none() => {}
            _ => return Err(AppError::validation("An attachment is missing or was already sent — attach it again.")),
        }
    }
    Ok(())
}

/// Give tool captures to the conversation they were taken in.
pub fn adopt(conn: &Connection, ids: &[String], conversation_id: &str) -> AppResult<()> {
    for id in ids {
        conn.execute("UPDATE attachments SET conversation_id = ?1 WHERE id = ?2 AND conversation_id IS NULL", params![conversation_id, id])?;
    }
    Ok(())
}

/// All attachments of a conversation's messages, keyed by message id.
pub fn by_message(conn: &Connection, conversation_id: &str) -> AppResult<std::collections::HashMap<String, Vec<Attachment>>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM attachments WHERE conversation_id = ?1 AND message_id IS NOT NULL ORDER BY created_at, rowid"))?;
    let mut map: std::collections::HashMap<String, Vec<Attachment>> = std::collections::HashMap::new();
    for a in stmt.query_map([conversation_id], row)? {
        let a = a?;
        map.entry(a.message_id.clone().unwrap_or_default()).or_default().push(a);
    }
    Ok(map)
}

#[cfg(test)]
pub mod testdata {
    /// A real 2×1 PNG.
    pub fn png() -> Vec<u8> {
        let mut out = Vec::new();
        let img = image::RgbImage::from_pixel(2, 1, image::Rgb([200, 30, 30]));
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// A minimal one-page PDF containing "Hello IGRIS".
    pub fn pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
            "<< /Length 41 >>\nstream\nBT /F1 18 Tf 20 40 Td (Hello IGRIS) Tj ET\nendstream",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in offsets {
            pdf.extend(format!("{off:010} 00000 n \n").as_bytes());
        }
        pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        pdf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup() -> (Database, AttachmentStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(dir.path().join("attachments")).unwrap();
        (Database::open_in_memory().unwrap(), store, dir)
    }

    #[test]
    fn sniffs_by_content_not_name() {
        assert_eq!(sniff(&testdata::png()), Some((MediaKind::Image, "image/png")));
        assert_eq!(sniff(&testdata::pdf()), Some((MediaKind::Pdf, "application/pdf")));
        assert_eq!(sniff(b"MZ\x90\x00 not an image"), None);
        assert_eq!(clean_name("C:\\Users\\a\\..\\secret\u{7}.png", "x"), "secret.png");
        assert_eq!(clean_name("   ", "image"), "image");
    }

    #[test]
    fn stages_links_and_hydrates() {
        let (db, store, _d) = setup();
        let mut conn = db.conn().unwrap();
        let img = store.save_upload(&conn, "photo.png", &testdata::png()).unwrap();
        assert_eq!((img.width, img.height, img.kind), (Some(2), Some(1), MediaKind::Image));
        let pdf = store.save_upload(&conn, "notes.pdf", &testdata::pdf()).unwrap();
        assert!(store.save_upload(&conn, "evil.png", b"#!/bin/sh\nrm -rf /").is_err());

        let c = crate::conversations::create(&conn, "t", "sys", &[]).unwrap();
        let m = crate::conversations::append(&mut conn, &c.id, crate::conversations::NewMessage::user("look")).unwrap();
        check_staged(&conn, &[img.id.clone(), pdf.id.clone()]).unwrap();
        assert!(check_staged(&conn, &[img.id.clone(), img.id.clone()]).is_err());
        link_to_message(&conn, &[img.id.clone(), pdf.id.clone()], &c.id, &m.id).unwrap();
        assert!(check_staged(&conn, std::slice::from_ref(&img.id)).is_err(), "can't be sent twice");
        assert_eq!(by_message(&conn, &c.id).unwrap()[&m.id].len(), 2);

        let mut media = pdf.media();
        store.hydrate(&conn, &mut media);
        assert!(media.data.is_some());
        assert!(media.text.as_deref().unwrap_or_default().contains("Hello IGRIS"), "{:?}", media.text);
    }

    #[test]
    fn rejects_oversized_and_discards_staged() {
        let (db, store, _d) = setup();
        let conn = db.conn().unwrap();
        let mut big = testdata::png();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        assert!(store.save_upload(&conn, "big.png", &big).unwrap_err().to_string().contains("at most"));
        let a = store.save_upload(&conn, "a.png", &testdata::png()).unwrap();
        store.discard(&conn, &a.id).unwrap();
        assert!(get(&conn, &a.id).unwrap().is_none());
        assert_eq!(std::fs::read_dir(store.dir()).unwrap().count(), 0);
    }

    #[test]
    fn gc_removes_files_of_deleted_conversations_and_stale_uploads() {
        let (db, store, _d) = setup();
        let mut conn = db.conn().unwrap();
        let kept = store.save_upload(&conn, "a.png", &testdata::png()).unwrap();
        let sent = store.save_upload(&conn, "b.png", &testdata::png()).unwrap();
        let stale = store.save_upload(&conn, "c.png", &testdata::png()).unwrap();
        conn.execute("UPDATE attachments SET created_at = '2000-01-01T00:00:00Z' WHERE id = ?1", [&stale.id]).unwrap();
        let c = crate::conversations::create(&conn, "t", "sys", &[]).unwrap();
        let m = crate::conversations::append(&mut conn, &c.id, crate::conversations::NewMessage::user("x")).unwrap();
        link_to_message(&conn, std::slice::from_ref(&sent.id), &c.id, &m.id).unwrap();

        crate::conversations::delete(&conn, &c.id).unwrap();
        assert!(get(&conn, &sent.id).unwrap().is_none(), "row cascades with the conversation");
        assert_eq!(store.gc(&conn).unwrap(), 2);
        assert!(get(&conn, &kept.id).unwrap().is_some());
        assert_eq!(std::fs::read_dir(store.dir()).unwrap().count(), 1);
    }
}
