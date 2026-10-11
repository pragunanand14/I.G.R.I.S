//! Attaching images and PDFs to chat messages.

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::State;

use crate::attachments::{self, Attachment};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Largest upload accepted over the IPC, before the type-specific limits.
const MAX_UPLOAD: usize = attachments::MAX_PDF_BYTES;

/// The file in an upload: `{ name, data }` with the bytes as base64 (works on
/// every platform's IPC), or — from older callers — the raw bytes with the
/// percent-encoded name in an `x-file-name` header. The name is for display
/// only; the type is sniffed from the bytes.
fn upload(request: &Request<'_>) -> AppResult<(String, Vec<u8>)> {
    match request.body() {
        InvokeBody::Json(v) => from_json(v),
        InvokeBody::Raw(bytes) => {
            let name = request
                .headers()
                .get("x-file-name")
                .and_then(|v| v.to_str().ok())
                .map(|v| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
                .unwrap_or_default();
            Ok((name, bytes.clone()))
        }
    }
}

fn from_json(v: &serde_json::Value) -> AppResult<(String, Vec<u8>)> {
    let data = v["data"].as_str().ok_or_else(|| AppError::validation("The file didn't come through. Try attaching it again."))?;
    if data.len() > MAX_UPLOAD / 3 * 4 + 8 {
        return Err(AppError::validation("That file is too big to attach (PDFs up to 20 MB, images up to 5 MB)."));
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| AppError::validation("The file didn't come through intact. Try again."))?;
    Ok((v["name"].as_str().unwrap_or_default().to_string(), bytes))
}

/// Stage an upload (see [`upload`] for the body).
#[tauri::command]
pub async fn attach_file(state: State<'_, AppState>, request: Request<'_>) -> AppResult<Attachment> {
    let (name, bytes) = upload(&request)?;
    let (db, store) = (state.db.clone(), state.attachments.clone());
    // PDF text extraction can take a moment; keep it off the async runtime.
    let a = tokio::task::spawn_blocking(move || store.save_upload(&*db.conn()?, &name, &bytes)).await.map_err(|e| AppError::internal(e.to_string()))??;
    tracing::info!(event = "ATTACHMENT_STAGED", kind = ?a.kind, size = a.size);
    Ok(a)
}

/// Remove a staged upload before sending.
#[tauri::command]
pub fn discard_attachment(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.attachments.discard(&*state.db.conn()?, &id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentData {
    pub mime: String,
    pub data: String,
}

/// File bytes (base64) for previews.
#[tauri::command]
pub fn read_attachment(state: State<'_, AppState>, id: String) -> AppResult<AttachmentData> {
    let a = attachments::get(&*state.db.conn()?, &id)?.ok_or_else(|| AppError::validation("That attachment no longer exists."))?;
    Ok(AttachmentData { data: state.attachments.read_base64(&a)?, mime: a.mime })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uploads_arrive_as_base64_json_on_every_platform() {
        let (name, bytes) = from_json(&json!({"name": "notes.pdf", "data": "JVBERi0xLjQ="})).unwrap();
        assert_eq!((name.as_str(), bytes.as_slice()), ("notes.pdf", &b"%PDF-1.4"[..]));
        assert!(from_json(&json!({"name": "x"})).is_err());
        assert!(from_json(&json!({"name": "x", "data": "not base64!"})).is_err());
        let huge = "A".repeat(MAX_UPLOAD / 3 * 4 + 100);
        assert!(from_json(&json!({"name": "x", "data": huge})).unwrap_err().to_string().contains("too big"));
    }
}
