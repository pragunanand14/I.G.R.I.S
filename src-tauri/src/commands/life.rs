//! People and Spending: the people IGRIS knows (add, change, forget) and
//! what the user spent (totals, fixing a category, deleting, scanning now,
//! and the "track by itself" switch).

use chrono::Local;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::commands::google::GoogleState;
use crate::error::AppResult;
use crate::expenses::{self, Expense, PhoneFeed, Total};
use crate::people::{self, Change, Person, Upcoming};
use crate::state::AppState;

/// This phone's payment messages (none on a computer).
pub struct PaymentFeed(pub Option<PhoneFeed>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeopleView {
    pub people: Vec<Person>,
    pub upcoming: Vec<Upcoming>,
}

#[tauri::command]
pub fn people_list(state: State<'_, AppState>) -> AppResult<PeopleView> {
    let conn = state.db.conn()?;
    Ok(PeopleView { people: people::list(&conn)?, upcoming: people::upcoming_birthdays(&conn, Local::now().date_naive(), 30)? })
}

#[tauri::command]
pub fn person_save(state: State<'_, AppState>, name: String, relation: String, birthday: String, notes: String) -> AppResult<Person> {
    people::save(&*state.db.conn()?, &name, Change { relation: Some(&relation), birthday: Some(&birthday), notes: Some(&notes) })
}

#[tauri::command]
pub fn person_forget(state: State<'_, AppState>, name: String) -> AppResult<()> {
    people::forget(&*state.db.conn()?, &name)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendingView {
    pub from: String,
    pub to: String,
    pub totals: Vec<Total>,
    pub items: Vec<Expense>,
    pub auto: bool,
    /// Where payments can come from on this device.
    pub phone: bool,
    pub email: bool,
}

#[tauri::command]
pub fn spending_view(state: State<'_, AppState>, feed: State<'_, PaymentFeed>, g: State<'_, GoogleState>, period: String) -> AppResult<SpendingView> {
    let conn = state.db.conn()?;
    let (from, to) = expenses::period(&period, Local::now().date_naive())?;
    Ok(SpendingView {
        from: from.to_string(),
        to: to.to_string(),
        totals: expenses::totals(&conn, from, to)?,
        items: expenses::list(&conn, from, to)?,
        auto: crate::settings::get_extra::<bool>(&conn, expenses::AUTO_KEY)?.unwrap_or(false),
        phone: feed.0.is_some(),
        email: g.0.is_connected(),
    })
}

#[tauri::command]
pub fn expense_set_category(state: State<'_, AppState>, id: i64, category: String) -> AppResult<Expense> {
    expenses::set_category(&*state.db.conn()?, id, &category)
}

#[tauri::command]
pub fn expense_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    expenses::delete(&*state.db.conn()?, id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub added: usize,
    pub notes: Vec<String>,
}

#[tauri::command]
pub async fn expenses_scan(app: AppHandle, state: State<'_, AppState>, feed: State<'_, PaymentFeed>, g: State<'_, GoogleState>) -> AppResult<ScanResult> {
    let r = expenses::scan_all(&state.db, &g.0, feed.0.clone(), 30).await?;
    let _ = app.emit("expenses-changed", ());
    Ok(ScanResult { added: r.added.len(), notes: r.notes })
}

#[tauri::command]
pub fn expenses_set_auto(state: State<'_, AppState>, on: bool) -> AppResult<()> {
    crate::settings::set_extra(&*state.db.conn()?, expenses::AUTO_KEY, &on)
}

/// While IGRIS runs and the user turned it on: look for new payments every
/// few hours (on the phone, message notifications only stay a day).
pub fn watch(app: AppHandle, db: std::sync::Arc<crate::db::Database>, google: std::sync::Arc<crate::google::Google>, feed: Option<PhoneFeed>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        loop {
            let on = db.conn().ok().and_then(|c| crate::settings::get_extra::<bool>(&c, expenses::AUTO_KEY).ok().flatten()).unwrap_or(false);
            if on {
                match expenses::scan_all(&db, &google, feed.clone(), 2).await {
                    Ok(r) if !r.added.is_empty() => {
                        let _ = app.emit("expenses-changed", ());
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(event = "EXPENSE_SCAN_FAILED", error = %e),
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3 * 3600)).await;
        }
    });
}
