//! Productivity: tasks, reminders and timers, a local calendar, and the
//! scheduler that fires reminders.

pub mod events;
pub mod reminders;
pub mod tasks;
pub mod time;

use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;

use crate::db::Database;
use crate::error::{AppError, AppResult};
use reminders::Reminder;

pub(crate) fn clean(s: &str, max: usize, field: &str) -> AppResult<String> {
    let s = s.trim().to_string();
    if s.chars().count() > max {
        return Err(AppError::validation(format!("{field} must be at most {max} characters.")));
    }
    Ok(s)
}

/// Payload of the `reminder-fired` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiredReminder {
    #[serde(flatten)]
    pub reminder: Reminder,
    /// Went off more than a minute late — IGRIS wasn't running at the due time.
    pub late: bool,
}

pub type OnFire = Arc<dyn Fn(&FiredReminder) + Send + Sync>;

/// One scheduler step: fire everything due. Returns what fired.
pub fn tick(db: &Database, on_fire: &OnFire) -> AppResult<Vec<FiredReminder>> {
    let now = Utc::now();
    let due = reminders::take_due(&*db.conn()?, now)?;
    let fired: Vec<FiredReminder> = due
        .into_iter()
        .map(|r| {
            let late = time::from_db(&r.due_at).is_some_and(|d| (now - d).num_seconds() > 60);
            FiredReminder { reminder: r, late }
        })
        .collect();
    for f in &fired {
        tracing::info!(event = "REMINDER_FIRED", id = f.reminder.id, kind = %f.reminder.kind, late = f.late);
        on_fire(f);
    }
    Ok(fired)
}

/// Check for due reminders every second for the life of the app (the platform
/// spawns this on its runtime). Reminders that came due while IGRIS was closed
/// fire (marked late) on the first tick.
pub async fn run_scheduler(db: Arc<Database>, on_fire: OnFire) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let db = db.clone();
        let on_fire = on_fire.clone();
        match tokio::task::spawn_blocking(move || tick(&db, &on_fire)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!(event = "SCHEDULER_TICK_FAILED", error = %e),
            Err(e) => tracing::warn!(event = "SCHEDULER_TICK_PANICKED", error = %e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn tick_fires_due_items_once_and_flags_late_ones() {
        let db = Database::open_in_memory().unwrap();
        {
            let c = db.conn().unwrap();
            let r = reminders::add_reminder(&c, "Old", "in 1 minute").unwrap();
            // Pretend it came due an hour ago while the app was closed.
            c.execute("UPDATE reminders SET due_at = ?1 WHERE id = ?2", rusqlite::params![time::to_db(Utc::now() - chrono::Duration::hours(1)), r.id]).unwrap();
            reminders::add_reminder(&c, "Future", "in 1 hour").unwrap();
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let on_fire: OnFire = Arc::new(move |f: &FiredReminder| s.lock().unwrap().push((f.reminder.title.clone(), f.late)));
        assert_eq!(tick(&db, &on_fire).unwrap().len(), 1);
        assert!(tick(&db, &on_fire).unwrap().is_empty());
        assert_eq!(*seen.lock().unwrap(), [("Old".to_string(), true)]);
    }
}
