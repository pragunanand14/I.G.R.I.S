//! Reminders that ring on the phone even when IGRIS is closed.
//!
//! Android stops IGRIS's own scheduler when the app is swiped away, so every
//! upcoming reminder and timer is also handed to Android as an exact alarm
//! (the notification plugin's scheduled notifications, which survive the app
//! being closed and the phone restarting). This keeps those alarms in step
//! with the reminders: new ones are scheduled, snoozed ones moved, cancelled
//! or dismissed ones removed. When IGRIS is open its in-app alert still shows.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chrono::Utc;
use tauri::AppHandle;
use tauri_plugin_notification::{NotificationExt, Schedule};

use crate::db::Database;
use crate::productivity::{reminders, time};

/// Scheduled alarm ids are this plus the reminder id, so they never collide
/// with IGRIS's other (random-id) notifications.
const ID_BASE: i64 = 0x4000_0000;
const CHANNEL: &str = "reminders";
const CHECK_EVERY: Duration = Duration::from_secs(3);

/// Reminders currently scheduled with Android (id → due), so the in-app
/// scheduler doesn't show a second notification for them.
fn scheduled() -> &'static Mutex<HashMap<i64, String>> {
    static S: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Was this reminder handed to Android (which notifies on its own)?
pub fn is_scheduled(id: i64) -> bool {
    scheduled().lock().map(|s| s.contains_key(&id)).unwrap_or(false)
}

fn alarm_id(reminder: i64) -> Option<i32> {
    i32::try_from(ID_BASE + reminder).ok()
}

/// Ask for the notification permission (Android asks the user once; after
/// that it's remembered), set up the reminders channel, then keep alarms in
/// step with the reminders for the life of the app.
pub fn start(app: AppHandle, db: Arc<Database>) {
    tauri::async_runtime::spawn(async move {
        let a = app.clone();
        let ready = tauri::async_runtime::spawn_blocking(move || prepare(&a)).await.unwrap_or(false);
        if !ready {
            return;
        }
        let mut last: Option<Vec<(i64, String)>> = None;
        loop {
            let (a, d, prev) = (app.clone(), db.clone(), last.clone());
            match tauri::async_runtime::spawn_blocking(move || sync(&a, &d, prev)).await {
                Ok(Ok(now)) => last = Some(now),
                Ok(Err(e)) => tracing::warn!(event = "PHONE_ALARMS_SYNC_FAILED", error = %e),
                Err(e) => tracing::warn!(event = "PHONE_ALARMS_SYNC_PANICKED", error = %e),
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

fn prepare(app: &AppHandle) -> bool {
    use tauri::plugin::PermissionState;
    use tauri_plugin_notification::{Channel, Importance, Visibility};
    let n = app.notification();
    let granted = matches!(n.permission_state(), Ok(PermissionState::Granted)) || matches!(n.request_permission(), Ok(PermissionState::Granted));
    if !granted {
        tracing::warn!(event = "PHONE_ALARMS_NO_PERMISSION");
        return false;
    }
    let channel = Channel::builder(CHANNEL, "Reminders and timers")
        .description("Your IGRIS reminders and timers")
        .importance(Importance::High)
        .visibility(Visibility::Private)
        .vibration(true)
        .build();
    if let Err(e) = n.create_channel(channel) {
        tracing::warn!(event = "PHONE_ALARMS_CHANNEL_FAILED", error = %e);
    }
    tracing::info!(event = "PHONE_ALARMS_READY");
    true
}

/// One pass: schedule what's upcoming, drop what isn't. Does nothing when
/// the upcoming list hasn't changed since `last`.
fn sync(app: &AppHandle, db: &Database, last: Option<Vec<(i64, String)>>) -> Result<Vec<(i64, String)>, String> {
    let now = Utc::now();
    let all = reminders::active(&*db.conn().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let upcoming: Vec<_> = all.into_iter().filter(|r| r.status == "pending" && time::from_db(&r.due_at).is_some_and(|d| d > now)).collect();
    let key: Vec<(i64, String)> = upcoming.iter().map(|r| (r.id, r.due_at.clone())).collect();
    if last.as_ref() == Some(&key) {
        return Ok(key);
    }

    let n = app.notification();
    let want: HashSet<i32> = upcoming.iter().filter_map(|r| alarm_id(r.id)).collect();
    let stale: Vec<i32> = n.pending().map_err(|e| e.to_string())?.iter().map(|p| p.id()).filter(|id| i64::from(*id) >= ID_BASE && !want.contains(id)).collect();
    if !stale.is_empty() {
        n.cancel(stale).map_err(|e| e.to_string())?;
    }

    let mut done = HashMap::new();
    for r in &upcoming {
        let (Some(id), Some(due)) = (alarm_id(r.id), time::from_db(&r.due_at)) else { continue };
        let Ok(date) = ::time::OffsetDateTime::from_unix_timestamp(due.timestamp()) else { continue };
        let heading = if r.kind == "timer" { "Timer finished" } else { "Reminder" };
        // Scheduling an id again replaces the earlier alarm (e.g. after a snooze).
        let res = n
            .builder()
            .id(id)
            .channel_id(CHANNEL)
            .title(format!("IGRIS · {heading}"))
            .body(r.title.clone())
            .auto_cancel()
            .schedule(Schedule::At { date, repeating: false, allow_while_idle: true })
            .show();
        match res {
            Ok(()) => {
                done.insert(r.id, r.due_at.clone());
            }
            Err(e) => tracing::warn!(event = "PHONE_ALARM_SCHEDULE_FAILED", id = r.id, error = %e),
        }
    }
    tracing::info!(event = "PHONE_ALARMS_SYNCED", scheduled = done.len());
    // Keep marks for reminders that just fired, so the in-app tick doesn't notify a second time.
    if let Ok(mut s) = scheduled().lock() {
        s.retain(|id, due| time::from_db(due).is_some_and(|d| d <= now && (now - d).num_minutes() < 10) || done.contains_key(id));
        s.extend(done);
    }
    Ok(key)
}
