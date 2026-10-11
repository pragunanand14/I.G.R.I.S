//! Settings → Accounts: connect or disconnect the user's Google account.
//! The UI only ever sees whether it's set up and the account's email.

use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::google::{Google, Opener, Status};

pub struct GoogleState(pub Arc<Google>);

#[tauri::command]
pub fn google_status(g: State<'_, GoogleState>) -> AppResult<Status> {
    g.0.status()
}

/// Opens Google's sign-in in the browser and waits for the user (up to 5 minutes).
#[tauri::command]
pub async fn google_connect(app: tauri::AppHandle, g: State<'_, GoogleState>) -> AppResult<Status> {
    let open: Opener = {
        #[cfg(desktop)]
        {
            let _ = &app;
            Arc::new(|u: &str| tauri_plugin_opener::open_url(u, None::<&str>).map_err(|e| e.to_string()))
        }
        #[cfg(mobile)]
        {
            use tauri_plugin_opener::OpenerExt;
            let app = app.clone();
            Arc::new(move |u: &str| app.opener().open_url(u, None::<&str>).map_err(|e| e.to_string()))
        }
    };
    g.0.connect(&open).await?;
    g.0.status()
}

#[tauri::command]
pub async fn google_disconnect(g: State<'_, GoogleState>) -> AppResult<Status> {
    g.0.disconnect().await?;
    g.0.status()
}
