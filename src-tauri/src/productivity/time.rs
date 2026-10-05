//! Natural-language times ("in 20 minutes", "tomorrow at 5pm", "friday 9am")
//! and durations ("1h 30m", "half an hour").
//!
//! Parsing is done here rather than by the model: the conversation's system
//! prompt is frozen with only the date, so relative times must be resolved
//! against the backend clock. The resolved time is always echoed back so the
//! user (and the model) can see exactly what was understood.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};
use regex::Regex;
use std::sync::LazyLock;

pub const EXAMPLES: &str = "Try e.g. \"in 20 minutes\", \"tomorrow at 5pm\", \"friday 9:30am\" or \"2026-10-06 17:00\".";

/// Storage format: UTC, second precision, lexicographically ordered.
pub fn to_db(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn from_db(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|t| t.with_timezone(&Utc))
}

pub fn now_db() -> String {
    to_db(Utc::now())
}

/// Resolve a local naive time to a real instant (DST gaps move forward an hour).
pub fn localize(t: NaiveDateTime) -> DateTime<Local> {
    Local
        .from_local_datetime(&t)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(t + Duration::hours(1))).earliest())
        .unwrap_or_else(|| Local.from_utc_datetime(&t))
}

/// Parse a future time relative to the local clock.
pub fn parse_when(input: &str) -> Result<DateTime<Utc>, String> {
    let now = Local::now();
    if let Ok(t) = DateTime::parse_from_rfc3339(input.trim()) {
        return future(t.with_timezone(&Utc), now.with_timezone(&Utc), input);
    }
    let naive = parse_when_naive(input, now.naive_local())?;
    future(localize(naive).with_timezone(&Utc), now.with_timezone(&Utc), input)
}

fn future(t: DateTime<Utc>, now: DateTime<Utc>, input: &str) -> Result<DateTime<Utc>, String> {
    if t <= now {
        return Err(format!("\"{}\" is in the past ({}).", input.trim(), describe(t)));
    }
    Ok(t)
}

/// Human-readable local time with the distance from now, e.g. "Tue 6 Oct, 17:00 (in 3 h 5 min)".
pub fn describe(t: DateTime<Utc>) -> String {
    let local = t.with_timezone(&Local);
    let now = Local::now();
    let date = if local.date_naive() == now.date_naive() {
        "today".to_string()
    } else if local.date_naive() == now.date_naive() + Duration::days(1) {
        "tomorrow".to_string()
    } else {
        local.format("%a %-d %b %Y").to_string()
    };
    let mut delta = t.signed_duration_since(Utc::now());
    // Round to the minute beyond the first minute, so "in 2 hours" doesn't read "in 1 h 59 min".
    if delta.num_seconds().abs() >= 60 {
        delta = Duration::minutes((delta.num_seconds() as f64 / 60.0).round() as i64);
    }
    let rel = if delta.num_seconds() >= 0 { format!("in {}", human_duration(delta)) } else { format!("{} ago", human_duration(-delta)) };
    format!("{date} at {} ({rel})", local.format("%H:%M"))
}

pub fn human_duration(d: Duration) -> String {
    let secs = d.num_seconds().max(0);
    let (days, hours, mins, s) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60, secs % 60);
    let parts: Vec<String> = [(days, "d"), (hours, "h"), (mins, "min")].iter().filter(|(n, _)| *n > 0).map(|(n, u)| format!("{n} {u}")).collect();
    if parts.is_empty() {
        format!("{s} s")
    } else if days == 0 && hours == 0 && s > 0 && mins < 5 {
        format!("{} {s} s", parts.join(" "))
    } else {
        parts.join(" ")
    }
}

static DURATION_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:and\s+)?(\d+(?:\.\d+)?|an?|half\s+an?|one|two|three|four|five|ten|fifteen|twenty|thirty|forty-five)\s*(seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|w)\s*")
        .unwrap()
});

fn word_number(s: &str) -> Option<f64> {
    Some(match s {
        "a" | "an" | "one" => 1.0,
        "two" => 2.0,
        "three" => 3.0,
        "four" => 4.0,
        "five" => 5.0,
        "ten" => 10.0,
        "fifteen" => 15.0,
        "twenty" => 20.0,
        "thirty" => 30.0,
        "forty-five" => 45.0,
        _ if s.starts_with("half") => 0.5,
        _ => s.parse().ok()?,
    })
}

fn unit_secs(u: &str) -> f64 {
    match u.chars().next() {
        Some('s') => 1.0,
        Some('m') => 60.0,
        Some('h') => 3600.0,
        Some('d') => 86_400.0,
        _ => 604_800.0, // weeks
    }
}

/// "1h 30m", "90 seconds", "an hour and a half", "half an hour", "2 days".
pub fn parse_duration(input: &str) -> Result<Duration, String> {
    let s = normalize(input);
    let mut rest = s.trim_start_matches("for ").trim();
    let mut total = 0.0;
    let mut last_unit = 0.0;
    let mut matched = false;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("and a half").or_else(|| rest.strip_prefix("and half")) {
            if last_unit == 0.0 {
                break;
            }
            total += last_unit / 2.0;
            rest = r.trim_start();
            continue;
        }
        let Some(c) = DURATION_PART.captures(rest) else { break };
        let n = word_number(&c[1]).ok_or_else(|| format!("I couldn't read the duration \"{}\".", input.trim()))?;
        last_unit = unit_secs(&c[2]);
        total += n * last_unit;
        matched = true;
        rest = &rest[c.get(0).unwrap().end()..];
    }
    if !matched || !rest.is_empty() {
        return Err(format!("I couldn't read the duration \"{}\". Try e.g. \"10 minutes\", \"1h 30m\" or \"half an hour\".", input.trim()));
    }
    let secs = total.round() as i64;
    if secs < 1 {
        return Err("The duration must be at least one second.".into());
    }
    if secs > 366 * 86_400 {
        return Err("That's more than a year away.".into());
    }
    Ok(Duration::seconds(secs))
}

fn normalize(input: &str) -> String {
    let s = input.to_lowercase().replace(['\u{2019}', '\''], "").replace(',', " ").replace("a.m.", "am").replace("p.m.", "pm");
    let s = s.trim().trim_end_matches(['.', '!', '?']).trim();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

static DAY_WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:on\s+)?(?:(this|next)\s+)?(the day after tomorrow|day after tomorrow|today|tonight|tomorrow|monday|tuesday|wednesday|thursday|friday|saturday|sunday|mon|tue|tues|wed|thu|thur|thurs|fri|sat|sun)\b").unwrap());
static ISO_DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:on\s+)?(\d{4})-(\d{2})-(\d{2})(?:t(\d{2}):(\d{2})(?::(\d{2}))?)?\b").unwrap());
static MONTH_DATE: LazyLock<Regex> = LazyLock::new(|| {
    let m = "(jan|january|feb|february|mar|march|apr|april|may|jun|june|jul|july|aug|august|sep|sept|september|oct|october|nov|november|dec|december)";
    Regex::new(&format!(r"\b(?:on\s+)?(?:the\s+)?(?:(\d{{1,2}})(?:st|nd|rd|th)?(?:\s+of)?\s+{m}|{m}\s+(?:the\s+)?(\d{{1,2}})(?:st|nd|rd|th)?)(?:\s+(\d{{4}}))?\b")).unwrap()
});
static CLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:at\s+|@\s*|by\s+)?(\d{1,2})(?:[:.](\d{2}))?\s*(am|pm|h)?\b").unwrap());
static PERIOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:at\s+|in\s+the\s+|this\s+)?(noon|midday|midnight|morning|afternoon|evening|night)\b").unwrap());
static RELATIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:in|after)\s+(.+?)(?:\s+from\s+now)?$|^(.+?)\s+from\s+now$").unwrap());

fn month_num(m: &str) -> u32 {
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|p| m.starts_with(p)).unwrap() as u32 + 1
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match &s[..3] {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

/// Pure parser over a naive local "now" (testable).
pub fn parse_when_naive(input: &str, now: NaiveDateTime) -> Result<NaiveDateTime, String> {
    let s = normalize(input);
    if s.is_empty() {
        return Err(format!("No time given. {EXAMPLES}"));
    }
    let fail = || format!("I couldn't understand the time \"{}\". {EXAMPLES}", input.trim());

    if let Some(c) = RELATIVE.captures(&s) {
        if let Ok(d) = parse_duration(c.get(1).or(c.get(2)).unwrap().as_str()) {
            return Ok(now + d);
        }
    }

    let mut rest = s.clone();
    let mut date: Option<NaiveDate> = None;
    let mut default_time = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let mut time: Option<NaiveTime> = None;
    // Day period words set a default time and disambiguate "at 5".
    let mut pm_hint: Option<bool> = None;

    if let Some(c) = ISO_DATE.captures(&s) {
        let (y, m, d) = (c[1].parse().unwrap(), c[2].parse().unwrap(), c[3].parse().unwrap());
        date = Some(NaiveDate::from_ymd_opt(y, m, d).ok_or_else(fail)?);
        if let (Some(h), Some(mi)) = (c.get(4), c.get(5)) {
            time = Some(NaiveTime::from_hms_opt(h.as_str().parse().unwrap(), mi.as_str().parse().unwrap(), 0).ok_or_else(fail)?);
        }
        rest = rest.replacen(c.get(0).unwrap().as_str(), " ", 1);
    } else if let Some(c) = MONTH_DATE.captures(&s) {
        let (day, month) = match (c.get(1), c.get(2), c.get(3), c.get(4)) {
            (Some(d), Some(m), _, _) => (d.as_str(), m.as_str()),
            (_, _, Some(m), Some(d)) => (d.as_str(), m.as_str()),
            _ => return Err(fail()),
        };
        let (d, m) = (day.parse().map_err(|_| fail())?, month_num(month));
        let explicit_year = c.get(5).map(|y| y.as_str().parse::<i32>().unwrap());
        let mut nd = NaiveDate::from_ymd_opt(explicit_year.unwrap_or(now.year()), m, d).ok_or_else(fail)?;
        if explicit_year.is_none() && nd < now.date() {
            nd = NaiveDate::from_ymd_opt(now.year() + 1, m, d).ok_or_else(fail)?;
        }
        date = Some(nd);
        rest = rest.replacen(c.get(0).unwrap().as_str(), " ", 1);
    } else if let Some(c) = DAY_WORDS.captures(&s) {
        let word = &c[2];
        let today = now.date();
        date = Some(match word {
            "today" => today,
            "tonight" => {
                pm_hint = Some(true);
                default_time = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
                today
            }
            "tomorrow" => today + Duration::days(1),
            w if w.contains("after tomorrow") => today + Duration::days(2),
            w => {
                let target = weekday(w).ok_or_else(fail)?;
                let mut ahead = (target.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
                if ahead == 0 {
                    ahead = 7;
                }
                today + Duration::days(ahead)
            }
        });
        rest = rest.replacen(c.get(0).unwrap().as_str(), " ", 1);
    }

    if let Some(c) = PERIOD.captures(&rest.clone()) {
        let (t, hint) = match &c[1] {
            "noon" | "midday" => (Some((12, 0)), None),
            "midnight" => (Some((0, 0)), None),
            "morning" => (None, Some(false)),
            "afternoon" | "evening" | "night" => (None, Some(true)),
            _ => (None, None),
        };
        default_time = match &c[1] {
            "morning" => NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            "afternoon" => NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
            "evening" => NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            "night" => NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            _ => default_time,
        };
        if let Some((h, m)) = t {
            time = Some(NaiveTime::from_hms_opt(h, m, 0).unwrap());
            if c[1] == *"midnight" && date.is_none() {
                date = Some(now.date() + Duration::days(1));
            }
        }
        pm_hint = hint.or(pm_hint);
        rest = rest.replacen(c.get(0).unwrap().as_str(), " ", 1);
    }

    let mut bare_hour: Option<(u32, u32)> = None;
    if time.is_none() {
        if let Some(c) = CLOCK.captures(&rest.clone()) {
            let h: u32 = c[1].parse().map_err(|_| fail())?;
            let m: u32 = c.get(2).map(|m| m.as_str().parse().unwrap()).unwrap_or(0);
            if m > 59 {
                return Err(fail());
            }
            let meridiem = c.get(3).map(|m| m.as_str());
            let hour = match meridiem {
                Some("am") if (1..=12).contains(&h) => h % 12,
                Some("pm") if (1..=12).contains(&h) => h % 12 + 12,
                Some("am") | Some("pm") => return Err(fail()),
                _ if h > 23 => return Err(fail()),
                _ if h >= 13 || h == 0 || c[1].starts_with('0') || meridiem == Some("h") => h,
                _ => match pm_hint {
                    Some(true) if h < 12 => h + 12,
                    Some(_) => h,
                    None => {
                        bare_hour = Some((h, m));
                        h
                    }
                },
            };
            time = Some(NaiveTime::from_hms_opt(hour, m, 0).ok_or_else(fail)?);
            rest = rest.replacen(c.get(0).unwrap().as_str(), " ", 1);
        }
    }

    // Anything left over that isn't filler means we didn't understand.
    let leftover: Vec<&str> = rest.split_whitespace().filter(|w| !["at", "on", "the", "of", "by", "o'clock", "oclock"].contains(w)).collect();
    if !leftover.is_empty() || (date.is_none() && time.is_none()) {
        return Err(fail());
    }

    let t = time.unwrap_or(default_time);
    match date {
        Some(d) => {
            // "tomorrow at 7" → 7pm, "friday at 9" → 9am: business-ish hours.
            let t = match bare_hour {
                Some((h, m)) if (1..=7).contains(&h) => NaiveTime::from_hms_opt(h + 12, m, 0).unwrap(),
                _ => t,
            };
            Ok(d.and_time(t))
        }
        None => {
            // Time only: the next time the clock shows it (for a bare hour, am or pm).
            let today = now.date();
            let mut candidates = vec![today.and_time(t), (today + Duration::days(1)).and_time(t)];
            if let Some((h, m)) = bare_hour {
                if h < 12 {
                    let pm = NaiveTime::from_hms_opt(h + 12, m, 0).unwrap();
                    candidates.push(today.and_time(pm));
                }
            }
            candidates.into_iter().filter(|c| *c > now).min().ok_or_else(fail)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Monday 5 October 2026, 10:30.
    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap().and_hms_opt(10, 30, 0).unwrap()
    }

    fn at(input: &str) -> String {
        parse_when_naive(input, now()).unwrap().format("%Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn relative_times() {
        assert_eq!(at("in 20 minutes"), "2026-10-05 10:50");
        assert_eq!(at("in an hour"), "2026-10-05 11:30");
        assert_eq!(at("in half an hour"), "2026-10-05 11:00");
        assert_eq!(at("in 1h 30m"), "2026-10-05 12:00");
        assert_eq!(at("in an hour and a half"), "2026-10-05 12:00");
        assert_eq!(at("in 2 days"), "2026-10-07 10:30");
        assert_eq!(at("10 minutes from now"), "2026-10-05 10:40");
        assert_eq!(at("In 5 mins."), "2026-10-05 10:35");
    }

    #[test]
    fn days_and_times() {
        assert_eq!(at("tomorrow at 5pm"), "2026-10-06 17:00");
        assert_eq!(at("tomorrow 9:15am"), "2026-10-06 09:15");
        assert_eq!(at("tomorrow"), "2026-10-06 09:00");
        assert_eq!(at("tomorrow morning"), "2026-10-06 09:00");
        assert_eq!(at("tomorrow evening"), "2026-10-06 18:00");
        assert_eq!(at("tomorrow at 7"), "2026-10-06 19:00");
        assert_eq!(at("tonight"), "2026-10-05 20:00");
        assert_eq!(at("tonight at 9"), "2026-10-05 21:00");
        assert_eq!(at("today at 17:45"), "2026-10-05 17:45");
        assert_eq!(at("friday 9:30am"), "2026-10-09 09:30");
        assert_eq!(at("on friday at 3 in the afternoon"), "2026-10-09 15:00");
        assert_eq!(at("next monday"), "2026-10-12 09:00"); // today is Monday → a week ahead
        assert_eq!(at("the day after tomorrow at noon"), "2026-10-07 12:00");
        assert_eq!(at("Wed, 2 p.m."), "2026-10-07 14:00");
    }

    #[test]
    fn time_only_picks_the_next_occurrence() {
        assert_eq!(at("at 5pm"), "2026-10-05 17:00");
        assert_eq!(at("at 9am"), "2026-10-06 09:00"); // already past today
        assert_eq!(at("at 5"), "2026-10-05 17:00"); // bare hour → next of 5:00 / 17:00
        assert_eq!(at("at 11"), "2026-10-05 11:00");
        assert_eq!(at("09:00"), "2026-10-06 09:00"); // zero-padded → 24h clock
        assert_eq!(at("noon"), "2026-10-05 12:00");
        assert_eq!(at("midnight"), "2026-10-06 00:00");
    }

    #[test]
    fn dates() {
        assert_eq!(at("2026-10-20"), "2026-10-20 09:00");
        assert_eq!(at("2026-10-20 14:30"), "2026-10-20 14:30");
        assert_eq!(at("2026-10-20T14:30"), "2026-10-20 14:30");
        assert_eq!(at("20 october at 6pm"), "2026-10-20 18:00");
        assert_eq!(at("oct 20th"), "2026-10-20 09:00");
        assert_eq!(at("on the 3rd of january"), "2027-01-03 09:00"); // already passed this year
        assert_eq!(at("march 1 2027 10am"), "2027-03-01 10:00");
    }

    #[test]
    fn rejects_nonsense() {
        for bad in ["", "whenever", "soonish", "at 25", "13pm", "tomorrow at banana", "feb 30", "in a while"] {
            assert!(parse_when_naive(bad, now()).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("10 minutes").unwrap(), Duration::minutes(10));
        assert_eq!(parse_duration("for 90 seconds").unwrap(), Duration::seconds(90));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("1.5 hours").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("two hours and 15 minutes").unwrap(), Duration::minutes(135));
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("ten bananas").is_err());
        assert!(parse_duration("0 seconds").is_err());
        assert!(parse_duration("400 days").is_err());
    }

    #[test]
    fn past_absolute_times_are_rejected() {
        assert!(parse_when("2020-01-01 10:00").unwrap_err().contains("in the past"));
        assert!(parse_when("in 5 minutes").is_ok());
    }

    #[test]
    fn db_format_round_trips_and_sorts() {
        let t = Utc.with_ymd_and_hms(2026, 10, 5, 9, 5, 7).unwrap();
        assert_eq!(to_db(t), "2026-10-05T09:05:07Z");
        assert_eq!(from_db(&to_db(t)), Some(t));
        assert!(to_db(t) < to_db(t + Duration::seconds(1)));
    }

    #[test]
    fn human_durations() {
        assert_eq!(human_duration(Duration::seconds(42)), "42 s");
        assert_eq!(human_duration(Duration::seconds(150)), "2 min 30 s");
        assert_eq!(human_duration(Duration::minutes(185)), "3 h 5 min");
        assert_eq!(human_duration(Duration::days(2) + Duration::hours(1)), "2 d 1 h");
    }
}
