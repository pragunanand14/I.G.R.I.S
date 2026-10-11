//! Knowing the user's life: the people in it (who they are to the user,
//! birthdays, notes) and what the user spends (picked out of payment
//! messages and emails on this device, or told directly).

use std::sync::Arc;

use chrono::{Local, NaiveDate};
use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::expenses::{self, PhoneFeed};
use crate::google::Google;
use crate::people::{self, Change, Person};

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: Vec::new() })
}

fn invalid(e: crate::error::AppError) -> ToolError {
    ToolError::invalid(e.to_string())
}

fn spec(name: &'static str, title: &'static str, description: &'static str, props: Value, required: &[&str], permission: PermissionLevel) -> ToolSpec {
    ToolSpec {
        name,
        title,
        description,
        input_schema: json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false }),
        permission,
    }
}

fn text<'a>(i: &'a Value, k: &str) -> Option<&'a str> {
    i[k].as_str()
}

fn person_line(p: &Person) -> String {
    let mut s = p.name.clone();
    if !p.relation.is_empty() {
        s.push_str(&format!(" ({})", p.relation));
    }
    if !p.birthday.is_empty() {
        s.push_str(&format!(" — birthday {}", p.birthday));
    }
    if !p.notes.is_empty() {
        s.push_str(&format!(" — {}", p.notes));
    }
    s
}

fn birthdays(db: &Database, days: i64) -> Result<String, ToolError> {
    let up = people::upcoming_birthdays(&*db.conn().map_err(invalid)?, Local::now().date_naive(), days).map_err(invalid)?;
    Ok(up
        .iter()
        .map(|u| {
            let when = match u.in_days {
                0 => "today".to_string(),
                1 => "tomorrow".to_string(),
                n => format!("in {n} days"),
            };
            let turns = u.turns.map(|t| format!(" (turns {t})")).unwrap_or_default();
            format!("{}'s birthday is {when}{turns}", u.person.name)
        })
        .collect::<Vec<_>>()
        .join("; "))
}

pub struct SavePersonTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl SavePersonTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: spec(
                "save_person",
                "Remember a person",
                "Remember someone in the user's life, or update what's known (only the fields given change): relation to the user \
(\"mom\", \"manager\", \"best friend\"), birthday (\"MM-DD\" or \"YYYY-MM-DD\"), notes (likes, dislikes, kids' names, gift ideas \
— short). Use it when the user tells you about someone. Never store passwords or ID numbers.",
                json!({
                    "name": { "type": "string", "minLength": 1, "maxLength": 60 },
                    "relation": { "type": "string", "maxLength": 60 },
                    "birthday": { "type": "string", "maxLength": 10 },
                    "notes": { "type": "string", "maxLength": 500 }
                }),
                &["name"],
                PermissionLevel::Low,
            ),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SavePersonTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Remember {}", text(i, "name").unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let c = Change { relation: text(i, "relation"), birthday: text(i, "birthday"), notes: text(i, "notes") };
        let p = people::save(&*self.db.conn().map_err(invalid)?, text(i, "name").unwrap_or_default(), c).map_err(invalid)?;
        out(format!("Saved: {}", person_line(&p)), format!("Remembered {}", p.name))
    }
}

pub struct FindPeopleTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl FindPeopleTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: spec(
                "find_people",
                "People",
                "Look up people in the user's life by name, relation or note (\"mom\", \"Rahul\", \"manager\"); \"\" lists everyone. \
Also says whose birthday is coming up. Use it when the user mentions someone (\"get something for my sister\") to know who they are.",
                json!({ "query": { "type": "string", "maxLength": 80 } }),
                &["query"],
                PermissionLevel::Safe,
            ),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for FindPeopleTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        match text(i, "query").unwrap_or_default().trim() {
            "" => "List the people IGRIS knows".into(),
            q => format!("Look up \"{q}\""),
        }
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let found = people::find(&*self.db.conn().map_err(invalid)?, text(i, "query").unwrap_or_default()).map_err(invalid)?;
        let mut s = if found.is_empty() {
            "Nobody matching is saved. If the user tells you who they are, save them with save_person.".to_string()
        } else {
            found.iter().take(30).map(person_line).collect::<Vec<_>>().join("\n")
        };
        let soon = birthdays(&self.db, 7)?;
        if !soon.is_empty() {
            s.push_str(&format!("\nComing up: {soon}."));
        }
        out(s, format!("{} person(s)", found.len()))
    }
}

pub struct ForgetPersonTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ForgetPersonTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: spec(
                "forget_person",
                "Forget a person",
                "Forget everything saved about someone (by exact name).",
                json!({ "name": { "type": "string", "minLength": 1, "maxLength": 60 } }),
                &["name"],
                PermissionLevel::Low,
            ),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ForgetPersonTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Forget {}", text(i, "name").unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let name = text(i, "name").unwrap_or_default();
        people::forget(&*self.db.conn().map_err(invalid)?, name).map_err(invalid)?;
        out(format!("Forgot {name}."), "Forgotten")
    }
}

fn default_currency(db: &Database) -> String {
    db.conn()
        .ok()
        .and_then(|c| c.query_row("SELECT currency FROM expenses GROUP BY currency ORDER BY COUNT(*) DESC LIMIT 1", [], |r| r.get(0)).ok())
        .unwrap_or_else(|| "INR".into())
}

pub struct LogExpenseTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl LogExpenseTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: spec(
                "log_expense",
                "Log an expense",
                "Record something the user says they spent (\"300 on lunch\"). amount in normal units (299.50). currency: 3-letter \
code, \"\" for the usual one. what: merchant or item. category: one of Food, Groceries, Transport, Shopping, Bills, Entertainment, \
Health, Transfers, Other (\"\" to guess). date: YYYY-MM-DD, \"\" for today.",
                json!({
                    "amount": { "type": "number", "exclusiveMinimum": 0, "maximum": 100000000 },
                    "currency": { "type": "string", "maxLength": 3 },
                    "what": { "type": "string", "minLength": 1, "maxLength": 60 },
                    "category": { "type": "string", "maxLength": 20 },
                    "date": { "type": "string", "maxLength": 10 }
                }),
                &["amount", "what"],
                PermissionLevel::Low,
            ),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for LogExpenseTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Log {} — {}", i["amount"], text(i, "what").unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let minor = (i["amount"].as_f64().unwrap_or_default() * 100.0).round() as i64;
        let currency = match text(i, "currency").unwrap_or_default().trim() {
            "" => default_currency(&self.db),
            c => c.to_string(),
        };
        let date = match text(i, "date").unwrap_or_default().trim() {
            "" => Local::now().date_naive(),
            d => NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| ToolError::invalid("date must be YYYY-MM-DD"))?,
        };
        let e = expenses::add_manual(
            &*self.db.conn().map_err(invalid)?,
            minor,
            &currency,
            text(i, "what").unwrap_or_default(),
            text(i, "category").unwrap_or_default(),
            date,
        )
        .map_err(invalid)?;
        out(
            format!("Logged #{}: {} — {} ({}) on {}.", e.id, expenses::money(e.amount_minor, &e.currency), e.merchant, e.category, e.spent_on),
            format!("Logged {}", expenses::money(e.amount_minor, &e.currency)),
        )
    }
}

const PERIODS: [&str; 7] = ["today", "this_week", "last_7_days", "this_month", "last_month", "last_30_days", "this_year"];

pub struct SpendingTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl SpendingTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: spec(
                "spending",
                "Spending",
                "What the user spent in a period: totals by category and the payments (newest first, with ids). category: only that \
one, \"\" for all. Covers what was found in payment messages/emails (scan_expenses) and what the user logged.",
                json!({
                    "period": { "type": "string", "enum": PERIODS },
                    "category": { "type": "string", "maxLength": 20 }
                }),
                &["period"],
                PermissionLevel::Safe,
            ),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SpendingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Your spending ({})", text(i, "period").unwrap_or("this_month").replace('_', " "))
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let conn = self.db.conn().map_err(invalid)?;
        let (from, to) = expenses::period(text(i, "period").unwrap_or("this_month"), Local::now().date_naive()).map_err(invalid)?;
        let cat = text(i, "category").unwrap_or_default().trim().to_lowercase();
        let keep = |c: &str| cat.is_empty() || c.eq_ignore_ascii_case(&cat);
        let totals: Vec<_> = expenses::totals(&conn, from, to).map_err(invalid)?.into_iter().filter(|t| keep(&t.category)).collect();
        if totals.is_empty() {
            return out(format!("No spending recorded from {from} to {to}. (scan_expenses looks for payment messages and emails.)"), "Nothing recorded");
        }
        let mut currencies: Vec<&str> = totals.iter().map(|t| t.currency.as_str()).collect();
        currencies.dedup();
        let mut s = format!("From {from} to {to}:\n");
        for cur in currencies {
            let sum: i64 = totals.iter().filter(|t| t.currency == cur).map(|t| t.amount_minor).sum();
            s.push_str(&format!("Total {}: ", expenses::money(sum, cur)));
            let parts: Vec<String> =
                totals.iter().filter(|t| t.currency == cur).map(|t| format!("{} {} ({})", t.category, expenses::money(t.amount_minor, cur), t.count)).collect();
            s.push_str(&parts.join(", "));
            s.push('\n');
        }
        for e in expenses::list(&conn, from, to).map_err(invalid)?.iter().filter(|e| keep(&e.category)).take(40) {
            let m = if e.merchant.is_empty() { "(unknown)" } else { &e.merchant };
            s.push_str(&format!("[#{}] {} {} — {} ({})\n", e.id, e.spent_on, expenses::money(e.amount_minor, &e.currency), m, e.category));
        }
        out(s.trim_end().to_string(), format!("{} categories", totals.len()))
    }
}

pub struct ScanExpensesTool {
    spec: ToolSpec,
    db: Arc<Database>,
    google: Arc<Google>,
    phone: Option<PhoneFeed>,
}

impl ScanExpensesTool {
    pub fn new(db: Arc<Database>, google: Arc<Google>, phone: Option<PhoneFeed>) -> Self {
        Self {
            spec: spec(
                "scan_expenses",
                "Find expenses",
                "Look through payment messages on the phone (the last day's bank/UPI SMS and alerts) and payment emails in Gmail (the \
last `days` days) and record the payments found. Each payment counts once however often you scan. Do it before answering \
about spending if the user hasn't scanned recently.",
                json!({ "days": { "type": "integer", "minimum": 1, "maximum": 60 } }),
                &["days"],
                PermissionLevel::Low,
            ),
            db,
            google,
            phone,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ScanExpensesTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Look for payments in your messages and emails".into()
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let days = i["days"].as_u64().unwrap_or(7).clamp(1, 60) as u32;
        if self.phone.is_none() && !self.google.is_connected() {
            return Err(ToolError::refused(
                "There's nothing to scan here: connect Google (Settings → Accounts) for payment emails, or use IGRIS on the phone for payment messages.",
            ));
        }
        let r = expenses::scan_all(&self.db, &self.google, self.phone.clone(), days).await.map_err(invalid)?;
        let mut s = format!("Looked at {} message(s): {} new payment(s), {} already recorded.", r.looked, r.added.len(), r.already_known);
        for e in r.added.iter().take(30) {
            let m = if e.merchant.is_empty() { "(unknown)" } else { &e.merchant };
            s.push_str(&format!("\n[#{}] {} {} — {} ({})", e.id, e.spent_on, expenses::money(e.amount_minor, &e.currency), m, e.category));
        }
        for n in &r.notes {
            s.push_str(&format!("\n{n}"));
        }
        out(s, format!("{} new payment(s)", r.added.len()))
    }
}

pub struct ChangeExpenseTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ChangeExpenseTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: spec(
                "change_expense",
                "Change an expense",
                "Fix an expense by id (from spending): action \"category\" with a new category, or \"delete\" (e.g. it wasn't a real \
expense, like moving money to your own account).",
                json!({
                    "id": { "type": "integer", "minimum": 1 },
                    "action": { "type": "string", "enum": ["category", "delete"] },
                    "category": { "type": "string", "maxLength": 20 }
                }),
                &["id", "action"],
                PermissionLevel::Low,
            ),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ChangeExpenseTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        match text(i, "action") {
            Some("delete") => format!("Delete expense #{}", i["id"]),
            _ => format!("Put expense #{} under {}", i["id"], text(i, "category").unwrap_or_default()),
        }
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let id = i["id"].as_i64().unwrap_or_default();
        let conn = self.db.conn().map_err(invalid)?;
        if text(i, "action") == Some("delete") {
            expenses::delete(&conn, id).map_err(invalid)?;
            return out(format!("Expense #{id} deleted."), "Deleted");
        }
        let e = expenses::set_category(&conn, id, text(i, "category").unwrap_or_default()).map_err(invalid)?;
        out(format!("Expense #{id} is now under {}.", e.category), format!("Now {}", e.category))
    }
}

/// The people tools and the money tools.
pub fn all(db: &Arc<Database>, google: &Arc<Google>, phone: Option<PhoneFeed>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(SavePersonTool::new(db.clone())),
        Arc::new(FindPeopleTool::new(db.clone())),
        Arc::new(ForgetPersonTool::new(db.clone())),
        Arc::new(LogExpenseTool::new(db.clone())),
        Arc::new(SpendingTool::new(db.clone())),
        Arc::new(ScanExpensesTool::new(db.clone(), google.clone(), phone)),
        Arc::new(ChangeExpenseTool::new(db.clone())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expenses::Found;

    #[tokio::test]
    async fn people_and_money_from_chat() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        // Google not connected: only the phone's messages are scanned.
        let google = Arc::new(crate::google::tests::google("http://127.0.0.1:9", db.clone()));
        let feed: PhoneFeed = Arc::new(|| {
            Ok(vec![
                Found { id: None, text: "Rs.250.00 debited from A/c XX1234 to VPA swiggy@icici".into(), date: Local::now().date_naive() },
                Found { id: None, text: "Mom: Dinner at 8?".into(), date: Local::now().date_naive() },
            ])
        });
        let [save, find, forget, log, spending, scan, change]: [Arc<dyn Tool>; 7] = all(&db, &google, Some(feed)).try_into().ok().unwrap();

        save.execute(&json!({"name": "Mom", "relation": "mother", "birthday": "03-04", "notes": "loves orchids"})).await.unwrap();
        assert!(find.execute(&json!({"query": "my mother"})).await.unwrap().content.contains("Mom (mother) — birthday 03-04 — loves orchids"));
        assert!(save.execute(&json!({"name": "Dad", "birthday": "yesterday"})).await.is_err());
        forget.execute(&json!({"name": "mom"})).await.unwrap();
        assert!(find.execute(&json!({"query": ""})).await.unwrap().content.starts_with("Nobody"));

        let s = scan.execute(&json!({"days": 7})).await.unwrap().content;
        assert!(s.contains("1 new payment(s)") && s.contains("₹250.00 — swiggy (Food)"), "{s}");
        assert!(scan.execute(&json!({"days": 7})).await.unwrap().content.contains("0 new payment(s), 1 already recorded"));
        log.execute(&json!({"amount": 99.5, "currency": "", "what": "Coffee", "category": "", "date": ""})).await.unwrap();
        let sp = spending.execute(&json!({"period": "today", "category": ""})).await.unwrap().content;
        assert!(sp.contains("Total ₹349.50: Food ₹349.50 (2)"), "{sp}");
        change.execute(&json!({"id": 1, "action": "category", "category": "Other"})).await.unwrap();
        change.execute(&json!({"id": 2, "action": "delete"})).await.unwrap();
        assert!(spending.execute(&json!({"period": "today", "category": "food"})).await.unwrap().content.starts_with("No spending"));
        assert_eq!(spending.spec().permission, PermissionLevel::Safe);
    }
}
