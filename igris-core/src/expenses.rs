//! What the user spends, without typing it in: payment messages (bank and
//! UPI SMS, card alerts) and payment emails are read on this device, the
//! amount and merchant picked out, and kept here. The user can also just say
//! "I spent 300 on lunch". Only the parsed amounts, merchants and categories
//! go to the AI, never the message text.
//!
//! Parsing is local and rule-based: it recognises common Indian bank / UPI
//! formats and card alerts in ₹, $, € and £. A message it isn't sure is a
//! payment (credits, OTPs, bills that are only due, failed payments) is
//! skipped rather than guessed.

use std::sync::OnceLock;

use chrono::{Datelike, Duration, Local, NaiveDate};
use regex::Regex;
use rusqlite::{params, Connection, Row};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::productivity::time;

/// Amounts at or above this (in minor units: 10 crore / 10 million) are not expenses IGRIS records.
const MAX_MINOR: i64 = 1_000_000_000;

pub const CATEGORIES: &[&str] = &["Food", "Groceries", "Transport", "Shopping", "Bills", "Entertainment", "Health", "Transfers", "Other"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expense {
    pub id: i64,
    /// In the currency's smallest unit (paise, cents).
    pub amount_minor: i64,
    pub currency: String,
    pub merchant: String,
    pub category: String,
    /// YYYY-MM-DD, local.
    pub spent_on: String,
    /// phone, email or manual.
    pub source: String,
}

/// A payment picked out of a message.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub amount_minor: i64,
    pub currency: String,
    pub merchant: String,
}

/// A message or email to look at: a stable id when there is one (email id),
/// the text, and the day it arrived.
#[derive(Debug, Clone)]
pub struct Found {
    pub id: Option<String>,
    pub text: String,
    pub date: NaiveDate,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

fn minor(digits: &str) -> Option<i64> {
    let d = digits.replace(',', "");
    let (int, frac) = d.split_once('.').unwrap_or((&d, ""));
    let int: i64 = int.parse().ok()?;
    let frac: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 10,
        _ => frac[..2].parse().ok()?,
    };
    int.checked_mul(100)?.checked_add(frac)
}

fn currency_of(sym: &str) -> &'static str {
    match sym.trim_end_matches('.').to_lowercase().as_str() {
        "$" | "usd" => "USD",
        "€" | "eur" => "EUR",
        "£" | "gbp" => "GBP",
        _ => "INR",
    }
}

/// The payment in a message, or `None` when it isn't clearly money going out.
pub fn parse(text: &str) -> Option<Parsed> {
    let t = text.to_lowercase();
    const NOT_SPENDING: &[&str] = &[
        "otp",
        "one time password",
        "verification code",
        "due on",
        "is due",
        "due date",
        "will be debited",
        "has requested",
        "requested money",
        "collect request",
        "minimum amount",
        "bill generated",
        "declined",
        "failed",
        "reversed",
        "refund",
        "cashback",
    ];
    if NOT_SPENDING.iter().any(|w| t.contains(w)) || (t.contains("credited") && !t.contains("debited")) {
        return None;
    }
    const SPENDING: &[&str] = &[
        "debited",
        "spent",
        "you paid",
        "paid to",
        "paid rs",
        "paid inr",
        "paid ₹",
        "sent rs",
        "sent inr",
        "sent ₹",
        "purchase",
        "withdrawn",
        "charged",
        "txn of",
        "payment of",
        "payment successful",
        "payment received by",
    ];
    if !SPENDING.iter().any(|w| t.contains(w)) {
        return None;
    }
    static AMOUNT: OnceLock<Regex> = OnceLock::new();
    let amount = re(&AMOUNT, r"(?i)(rs\.?|inr|₹|usd|\$|eur|€|gbp|£)\s*([0-9][0-9,]*(?:\.[0-9]{1,2})?)");
    let c = amount.captures(text)?;
    let amount_minor = minor(&c[2]).filter(|m| *m > 0 && *m < MAX_MINOR)?;
    Some(Parsed { amount_minor, currency: currency_of(&c[1]).into(), merchant: merchant(text) })
}

fn merchant(text: &str) -> String {
    static VPA: OnceLock<Regex> = OnceLock::new();
    static NAMED: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&VPA, r"(?i)\bvpa\s+([a-z0-9._-]+)@").captures(text) {
        return tidy(&c[1]);
    }
    let named =
        re(&NAMED, r"(?i)\b(?:at|to|towards)\s+([a-z0-9][a-z0-9 &'._*-]{1,40}?)(?:\s+(?:on|via|ref|using|from|with|avl|upi|thru|through|for)\b|[.,(:;]|$)");
    for c in named.captures_iter(text) {
        let m = c[1].trim();
        let low = m.to_lowercase();
        if ["your", "a/c", "ac ", "acct", "account", "card", "bank", "the "].iter().any(|w| low.starts_with(w))
            || low.chars().all(|c| c.is_ascii_digit() || c == '*' || c == 'x')
        {
            continue;
        }
        return tidy(m);
    }
    String::new()
}

fn tidy(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(40).collect();
    s.trim_matches(|c: char| !c.is_alphanumeric()).replace(['_', '*'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A best guess from the merchant (and message) — the user can change it.
pub fn category(merchant: &str, text: &str) -> &'static str {
    let m = format!("{} {}", merchant, text).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    // Whole words from the start ("tea" isn't "steam"); phrases anywhere.
    let merchant_low = merchant.to_lowercase();
    let tokens: Vec<&str> = merchant_low.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    let mhas = |words: &[&str]| words.iter().any(|w| if w.contains(' ') { merchant_low.contains(w) } else { tokens.iter().any(|t| t.starts_with(w)) });
    if mhas(&[
        "swiggy",
        "zomato",
        "restaurant",
        "cafe",
        "café",
        "domino",
        "pizza",
        "mcdonald",
        "starbucks",
        "kfc",
        "burger",
        "eatsure",
        "biryani",
        "chai",
        "coffee",
        "tea",
        "lunch",
        "dinner",
        "breakfast",
        "snack",
        "food",
    ]) {
        "Food"
    } else if mhas(&["blinkit", "zepto", "bigbasket", "dmart", "instamart", "grocer", "jiomart", "supermarket", "kirana", "fresh"]) {
        "Groceries"
    } else if mhas(&[
        "uber",
        "ola",
        "rapido",
        "irctc",
        "metro",
        "fuel",
        "petrol",
        "indianoil",
        "iocl",
        "hpcl",
        "bpcl",
        "shell",
        "fastag",
        "redbus",
        "indigo",
        "air india",
        "parking",
    ]) {
        "Transport"
    } else if mhas(&["amazon", "flipkart", "myntra", "ajio", "meesho", "nykaa", "decathlon", "ikea", "croma", "reliance digital"]) {
        "Shopping"
    } else if mhas(&["airtel", "jio", "vodafone", "electric", "bescom", "tata power", "adani", "broadband", "recharge", "gas", "water", "insurance", "lic"]) {
        "Bills"
    } else if mhas(&["netflix", "spotify", "hotstar", "prime video", "bookmyshow", "pvr", "inox", "youtube", "steam", "playstation"]) {
        "Entertainment"
    } else if mhas(&["pharm", "apollo", "medplus", "1mg", "pharmeasy", "hospital", "clinic", "diagnostic", "lab"]) {
        "Health"
    } else if has(&["sent rs", "sent inr", "sent ₹"])
        || (!merchant.is_empty() && !merchant.contains('.') && merchant.split_whitespace().count() >= 2 && has(&["upi"]))
    {
        "Transfers"
    } else {
        "Other"
    }
}

fn row(r: &Row) -> rusqlite::Result<Expense> {
    Ok(Expense {
        id: r.get(0)?,
        amount_minor: r.get(1)?,
        currency: r.get(2)?,
        merchant: r.get(3)?,
        category: r.get(4)?,
        spent_on: r.get(5)?,
        source: r.get(6)?,
    })
}

const COLS: &str = "id, amount_minor, currency, merchant, category, spent_on, source";

fn fingerprint(source: &str, key: &str) -> String {
    let h = Sha256::digest(format!("{source}\n{key}").as_bytes());
    h.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

fn normalise_category(c: &str) -> AppResult<&'static str> {
    CATEGORIES
        .iter()
        .find(|k| k.eq_ignore_ascii_case(c.trim()))
        .copied()
        .ok_or_else(|| AppError::validation(format!("Category must be one of: {}.", CATEGORIES.join(", "))))
}

/// Something the user told IGRIS they spent.
pub fn add_manual(conn: &Connection, amount_minor: i64, currency: &str, merchant: &str, category: &str, spent_on: NaiveDate) -> AppResult<Expense> {
    if amount_minor <= 0 || amount_minor >= MAX_MINOR {
        return Err(AppError::validation("That amount isn't valid."));
    }
    let currency = currency.trim().to_uppercase();
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::validation("Currency must be a 3-letter code like INR."));
    }
    let merchant = tidy(merchant);
    let category = if category.trim().is_empty() { self::category(&merchant, "") } else { normalise_category(category)? };
    let fp = fingerprint("manual", &uuid::Uuid::new_v4().to_string());
    conn.execute(
        "INSERT INTO expenses (amount_minor, currency, merchant, category, spent_on, source, fingerprint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6, ?7)",
        params![amount_minor, currency, merchant, category, spent_on.to_string(), fp, time::now_db()],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Expense> {
    conn.query_row(&format!("SELECT {COLS} FROM expenses WHERE id = ?1"), [id], row).map_err(|_| AppError::validation(format!("There's no expense #{id}.")))
}

/// What a scan found.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub added: Vec<Expense>,
    pub already_known: usize,
    pub not_payments: usize,
}

/// Record the payments in `items` (from `source`: "phone" or "email").
/// Each message counts once, however often it is scanned; a payment seen in
/// both an SMS and an email on the same day (same amount) counts once too.
pub fn record(conn: &Connection, source: &str, items: &[Found]) -> AppResult<Scan> {
    let mut scan = Scan::default();
    for f in items {
        let Some(p) = parse(&f.text) else {
            scan.not_payments += 1;
            continue;
        };
        let key = f.id.clone().unwrap_or_else(|| f.text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase());
        let fp = fingerprint(source, &key);
        let day = f.date.to_string();
        let seen: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM expenses WHERE fingerprint = ?1 \
             OR (amount_minor = ?2 AND currency = ?3 AND spent_on = ?4 AND source NOT IN (?5, 'manual')))",
            params![fp, p.amount_minor, p.currency, day, source],
            |r| r.get(0),
        )?;
        if seen {
            scan.already_known += 1;
            continue;
        }
        let cat = category(&p.merchant, &f.text);
        conn.execute(
            "INSERT INTO expenses (amount_minor, currency, merchant, category, spent_on, source, fingerprint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![p.amount_minor, p.currency, p.merchant, cat, day, source, fp, time::now_db()],
        )?;
        scan.added.push(get(conn, conn.last_insert_rowid())?);
    }
    if !scan.added.is_empty() {
        tracing::info!(event = "EXPENSES_RECORDED", source, count = scan.added.len());
    }
    Ok(scan)
}

pub fn set_category(conn: &Connection, id: i64, category: &str) -> AppResult<Expense> {
    let c = normalise_category(category)?;
    if conn.execute("UPDATE expenses SET category = ?1 WHERE id = ?2", params![c, id])? == 0 {
        return Err(AppError::validation(format!("There's no expense #{id}.")));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    if conn.execute("DELETE FROM expenses WHERE id = ?1", [id])? == 0 {
        return Err(AppError::validation(format!("There's no expense #{id}.")));
    }
    Ok(())
}

pub fn list(conn: &Connection, from: NaiveDate, to: NaiveDate) -> AppResult<Vec<Expense>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM expenses WHERE spent_on BETWEEN ?1 AND ?2 ORDER BY spent_on DESC, id DESC LIMIT 1000"))?;
    let rows = stmt.query_map(params![from.to_string(), to.to_string()], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// A named period ending today: today, this_week, last_7_days, this_month,
/// last_month, last_30_days, this_year.
pub fn period(name: &str, today: NaiveDate) -> AppResult<(NaiveDate, NaiveDate)> {
    let first_of_month = today.with_day(1).unwrap_or(today);
    Ok(match name {
        "today" => (today, today),
        "this_week" => (today - Duration::days(today.weekday().num_days_from_monday() as i64), today),
        "last_7_days" => (today - Duration::days(6), today),
        "this_month" => (first_of_month, today),
        "last_month" => {
            let end = first_of_month - Duration::days(1);
            (end.with_day(1).unwrap_or(end), end)
        }
        "last_30_days" => (today - Duration::days(29), today),
        "this_year" => (NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today), today),
        _ => return Err(AppError::validation("period must be today, this_week, last_7_days, this_month, last_month, last_30_days or this_year.")),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Total {
    pub currency: String,
    pub category: String,
    pub amount_minor: i64,
    pub count: i64,
}

/// Totals per currency and category, biggest first.
pub fn totals(conn: &Connection, from: NaiveDate, to: NaiveDate) -> AppResult<Vec<Total>> {
    let mut stmt = conn.prepare(
        "SELECT currency, category, SUM(amount_minor), COUNT(*) FROM expenses WHERE spent_on BETWEEN ?1 AND ?2 \
         GROUP BY currency, category ORDER BY currency, SUM(amount_minor) DESC",
    )?;
    let rows = stmt
        .query_map(params![from.to_string(), to.to_string()], |r| {
            Ok(Total { currency: r.get(0)?, category: r.get(1)?, amount_minor: r.get(2)?, count: r.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// "₹1,299.50", "$12.99", "AED 40.00".
pub fn money(amount_minor: i64, currency: &str) -> String {
    let (int, frac) = (amount_minor / 100, amount_minor % 100);
    let digits = int.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sym = match currency {
        "INR" => "₹",
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        other => return format!("{other} {grouped}.{frac:02}"),
    };
    format!("{sym}{grouped}.{frac:02}")
}

/// Payment emails from the last `days` days (the subject and preview are enough to read the amount).
pub async fn from_gmail(g: &crate::google::Google, days: u32) -> AppResult<Vec<Found>> {
    let q = format!(
        "newer_than:{}d {{debited spent \"you paid\" \"payment successful\" charged \"transaction alert\" \"txn\"}} -category:promotions",
        days.clamp(1, 60)
    );
    let today = Local::now().date_naive();
    Ok(crate::google::gmail::search(g, &q, crate::google::gmail::MAX_RESULTS)
        .await?
        .into_iter()
        .map(|m| Found {
            date: chrono::DateTime::parse_from_rfc2822(&m.date).map(|d| d.with_timezone(&Local).date_naive()).unwrap_or(today),
            text: format!("{} — {}", m.subject, m.snippet),
            id: Some(m.id),
        })
        .collect())
}

/// Payment-looking messages on this phone (recent notifications); none on a computer.
pub type PhoneFeed = std::sync::Arc<dyn Fn() -> Result<Vec<Found>, String> + Send + Sync>;

/// What a scan of every source found.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub looked: usize,
    pub added: Vec<Expense>,
    pub already_known: usize,
    /// Sources that couldn't be read, and why.
    pub notes: Vec<String>,
}

/// Scan the phone's messages (if any) and Gmail (if connected, the last `days` days).
pub async fn scan_all(db: &crate::db::Database, google: &crate::google::Google, phone: Option<PhoneFeed>, days: u32) -> AppResult<Report> {
    let mut r = Report::default();
    let take = |r: &mut Report, source: &str, items: Vec<Found>| -> AppResult<()> {
        r.looked += items.len();
        let s = record(&*db.conn()?, source, &items)?;
        r.already_known += s.already_known;
        r.added.extend(s.added);
        Ok(())
    };
    if let Some(feed) = phone {
        match tokio::task::spawn_blocking(move || feed()).await.map_err(|e| AppError::internal(e.to_string()))? {
            Ok(items) => take(&mut r, "phone", items)?,
            Err(e) => r.notes.push(format!("Phone messages: {e}")),
        }
    }
    if google.is_connected() {
        match from_gmail(google, days).await {
            Ok(items) => take(&mut r, "email", items)?,
            Err(e) => r.notes.push(format!("Gmail: {e}")),
        }
    } else {
        r.notes.push("Gmail isn't connected (Settings → Accounts), so emails weren't checked.".into());
    }
    Ok(r)
}

/// The user's choice: look for payments by itself every few hours.
pub const AUTO_KEY: &str = "expensesAuto";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn p(t: &str) -> Option<(i64, String, String)> {
        parse(t).map(|p| (p.amount_minor, p.currency, p.merchant))
    }

    #[test]
    fn reads_payment_messages() {
        assert_eq!(
            p("Rs.250.00 debited from A/c XX1234 on 10-10-26 to VPA swiggy@icici (UPI Ref No 123456). Not you? Call 18002586161"),
            Some((25000, "INR".into(), "swiggy".into()))
        );
        assert_eq!(
            p("INR 1,299.00 spent on HDFC Bank Card XX1234 at AMAZON PAY INDIA on 2026-10-09:12:30:01. Avl Lmt: INR 50,000"),
            Some((129900, "INR".into(), "AMAZON PAY INDIA".into()))
        );
        assert_eq!(p("Sent Rs.500.00 From HDFC Bank A/C *1234 To Ramesh Kumar On 10/10/26 Ref 1234"), Some((50000, "INR".into(), "Ramesh Kumar".into())));
        assert_eq!(p("You paid ₹120 to Uber India"), Some((12000, "INR".into(), "Uber India".into())));
        assert_eq!(p("Your card ending 1234 was charged $12.99 at NETFLIX.COM"), Some((1299, "USD".into(), "NETFLIX".into())));
        assert_eq!(p("Your A/c XX1234 is credited with Rs 10,000 on 10-10-26"), None);
        assert_eq!(p("123456 is your OTP for txn of Rs 500 at Flipkart. Do not share"), None);
        assert_eq!(p("Payment of ₹649 due on 15 Oct for your Airtel bill"), None);
        assert_eq!(p("Lunch at 1?"), None);
        assert_eq!(category("swiggy", ""), "Food");
        assert_eq!(category("Steam Games", ""), "Entertainment");
        assert_eq!(category("Uber India", ""), "Transport");
        assert_eq!(category("Ramesh Kumar", "Sent Rs.500.00 To Ramesh Kumar"), "Transfers");
        assert_eq!(money(12345678, "INR"), "₹123,456.78");
        assert_eq!(money(1299, "USD"), "$12.99");
    }

    #[test]
    fn records_each_payment_once() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let sms = Found { id: None, text: "Rs.250.00 debited from A/c XX1234 to VPA swiggy@icici".into(), date: day };
        let otp = Found { id: None, text: "123456 is your OTP".into(), date: day };
        let s = record(&c, "phone", &[sms.clone(), otp]).unwrap();
        assert_eq!((s.added.len(), s.not_payments), (1, 1));
        assert_eq!(record(&c, "phone", &[sms]).unwrap().already_known, 1);
        // The same payment's email doesn't count twice.
        let email = Found { id: Some("e1".into()), text: "Payment successful — Rs 250.00 paid to Swiggy".into(), date: day };
        assert_eq!(record(&c, "email", &[email]).unwrap().already_known, 1);

        add_manual(&c, 30000, "inr", "Lunch with team", "food", day).unwrap();
        assert!(add_manual(&c, 100, "INR", "x", "Gadgets", day).is_err());
        let t = totals(&c, day, day).unwrap();
        assert_eq!(t, vec![Total { currency: "INR".into(), category: "Food".into(), amount_minor: 55000, count: 2 }]);
        let id = list(&c, day, day).unwrap()[0].id;
        assert_eq!(set_category(&c, id, "other").unwrap().category, "Other");
        delete(&c, id).unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap(); // a Sunday
        assert_eq!(period("this_week", today).unwrap().0, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());
        assert_eq!(period("last_month", today).unwrap(), (NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()));
    }
}
