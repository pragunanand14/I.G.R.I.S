//! Attaching images and PDFs to chat messages.

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::State;

use crate::attachments::{self, Attachment};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Stage an upload. The body is the raw file bytes; the `x-file-name` header
/// carries the percent-encoded name (display only — the type is sniffed).
#[tauri::command]
pub async fn attach_file(state: State<'_, AppState>, request: Request<'_>) -> AppResult<Attachment> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::validation("Expected the file's bytes."));
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(|v| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
        .unwrap_or_default();
    let (db, store, bytes) = (state.db.clone(), state.attachments.clone(), bytes.clone());
    // PDF text extraction can take a moment; keep it off the async runtime.
    let a = tokio::task::spawn_blocking(move || store.save_upload(&*db.conn()?, &name, &bytes))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
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
