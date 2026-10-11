//! When a routine runs: daily, on weekdays/weekends/chosen days, once, or
//! when an email matching a sender/search arrives. Times are local.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Daily,
    Weekdays,
    Weekends,
    /// On the listed `days`.
    Weekly,
    /// Once, on `date`.
    Once,
    /// When an email matching `email` arrives (Gmail search, e.g. "from:boss@work.com").
    Email,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Daily => "daily",
            Kind::Weekdays => "weekdays",
            Kind::Weekends => "weekends",
            Kind::Weekly => "weekly",
            Kind::Once => "once",
            Kind::Email => "email",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Schedule {
    pub kind: Kind,
    /// "HH:MM", 24-hour, local.
    #[serde(default)]
    pub time: String,
    /// For weekly: "mon".."sun".
    #[serde(default)]
    pub days: Vec<String>,
    /// For once: "YYYY-MM-DD".
    #[serde(default)]
    pub date: String,
    /// For email: a Gmail search ("from:rahul@example.com").
    #[serde(default)]
    pub email: String,
}

const DAYS: [(&str, Weekday); 7] = [
    ("mon", Weekday::Mon),
    ("tue", Weekday::Tue),
    ("wed", Weekday::Wed),
    ("thu", Weekday::Thu),
    ("fri", Weekday::Fri),
    ("sat", Weekday::Sat),
    ("sun", Weekday::Sun),
];

fn weekday(s: &str) -> Option<Weekday> {
    let s = s.trim().to_lowercase();
    DAYS.iter().find(|(n, _)| s.starts_with(n)).map(|(_, d)| *d)
}

impl Schedule {
    pub fn is_event(&self) -> bool {
        self.kind == Kind::Email
    }

    fn at(&self) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(self.time.trim(), "%H:%M").ok()
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.kind {
            Kind::Email => {
                if self.email.trim().is_empty() || self.email.len() > 200 {
                    return Err("Say which emails start it (e.g. from:boss@work.com).".into());
                }
            }
            _ => {
                self.at().ok_or("Give the time as HH:MM (24-hour), e.g. 08:00 or 21:30.")?;
                if self.kind == Kind::Weekly && (self.days.is_empty() || self.days.iter().any(|d| weekday(d).is_none())) {
                    return Err("Say which days (mon, tue, wed, thu, fri, sat, sun).".into());
                }
                if self.kind == Kind::Once {
                    NaiveDate::parse_from_str(self.date.trim(), "%Y-%m-%d").map_err(|_| "Give the date as YYYY-MM-DD.")?;
                }
            }
        }
        Ok(())
    }

    fn runs_on(&self, d: Weekday) -> bool {
        match self.kind {
            Kind::Daily => true,
            Kind::Weekdays => !matches!(d, Weekday::Sat | Weekday::Sun),
            Kind::Weekends => matches!(d, Weekday::Sat | Weekday::Sun),
            Kind::Weekly => self.days.iter().filter_map(|s| weekday(s)).any(|w| w == d),
            Kind::Once | Kind::Email => false,
        }
    }

    /// The next run strictly after `after`; `None` for email triggers and past one-offs.
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let at = self.at()?;
        let local = |d: NaiveDate| Local.from_local_datetime(&d.and_time(at)).earliest().map(|t| t.with_timezone(&Utc));
        match self.kind {
            Kind::Email => None,
            Kind::Once => local(NaiveDate::parse_from_str(self.date.trim(), "%Y-%m-%d").ok()?).filter(|t| *t > after),
            _ => {
                let today = after.with_timezone(&Local).date_naive();
                (0..8).map(|i| today + Duration::days(i)).filter(|d| self.runs_on(d.weekday())).filter_map(local).find(|t| *t > after)
            }
        }
    }

    /// In plain words: "Every weekday at 08:00".
    pub fn describe(&self) -> String {
        let t = self.time.trim();
        match self.kind {
            Kind::Daily => format!("Every day at {t}"),
            Kind::Weekdays => format!("Every weekday at {t}"),
            Kind::Weekends => format!("Weekends at {t}"),
            Kind::Weekly => {
                let days: Vec<String> = self.days.iter().filter_map(|d| weekday(d)).map(|w| w.to_string()).collect();
                format!("Every {} at {t}", days.join(", "))
            }
            Kind::Once => format!("Once, on {} at {t}", self.date.trim()),
            Kind::Email => format!("When an email arrives matching \"{}\"", self.email.trim()),
        }
    }
}
