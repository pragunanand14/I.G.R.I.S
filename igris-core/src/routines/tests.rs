use std::sync::Mutex;

use chrono::{Duration, TimeZone, Utc};

use super::schedule::Kind;
use super::*;
use crate::db::Database;

fn sched(kind: Kind, time: &str) -> Schedule {
    Schedule { kind, time: time.into(), ..Default::default() }
}

#[derive(Default)]
struct Fake {
    ran: Mutex<Vec<String>>,
    fail: bool,
}

#[async_trait::async_trait]
impl Runner for Fake {
    fn open(&self, _r: &Routine, message: &str) -> AppResult<String> {
        self.ran.lock().unwrap().push(message.to_string());
        Ok(format!("conv-{}", self.ran.lock().unwrap().len()))
    }
    async fn run(&self, _r: &Routine, _c: &str) -> AppResult<String> {
        if self.fail {
            return Err(AppError::validation("AI isn't set up"));
        }
        Ok("Done: 3 meetings today.".into())
    }
}

#[test]
fn schedules_find_their_next_time() {
    // A Wednesday, 10:00 local.
    let wed = chrono::Local.with_ymd_and_hms(2026, 10, 7, 10, 0, 0).unwrap().with_timezone(&Utc);
    let local = |t: chrono::DateTime<Utc>| t.with_timezone(&chrono::Local).format("%a %H:%M").to_string();
    assert_eq!(local(sched(Kind::Daily, "08:00").next_after(wed).unwrap()), "Thu 08:00");
    assert_eq!(local(sched(Kind::Daily, "21:30").next_after(wed).unwrap()), "Wed 21:30");
    assert_eq!(local(sched(Kind::Weekends, "09:00").next_after(wed).unwrap()), "Sat 09:00");
    let fri = Schedule { kind: Kind::Weekly, time: "18:00".into(), days: vec!["fri".into(), "mon".into()], ..Default::default() };
    assert_eq!(local(fri.next_after(wed).unwrap()), "Fri 18:00");
    let weekday_late = chrono::Local.with_ymd_and_hms(2026, 10, 9, 23, 0, 0).unwrap().with_timezone(&Utc);
    assert_eq!(local(sched(Kind::Weekdays, "07:00").next_after(weekday_late).unwrap()), "Mon 07:00");
    let once = Schedule { kind: Kind::Once, time: "12:00".into(), date: "2026-10-08".into(), ..Default::default() };
    assert!(once.next_after(wed).is_some());
    assert!(once.next_after(wed + Duration::days(2)).is_none());
    assert!(sched(Kind::Daily, "8am").validate().is_err());
    assert!(Schedule { kind: Kind::Email, ..Default::default() }.validate().is_err());
    assert_eq!(fri.describe(), "Every Fri, Mon at 18:00");
}

#[tokio::test]
async fn due_routines_run_once_and_late_ones_are_skipped() {
    let db = Database::open_in_memory().unwrap();
    let r = create(&db.conn().unwrap(), "Morning", "Tell me the weather and my meetings", sched(Kind::Daily, "08:00")).unwrap();
    assert!(r.next_run_at.is_some() && r.enabled);
    let fake = Fake::default();

    // Not due yet.
    assert_eq!(tick(&db, &fake, Utc::now()).await.unwrap(), 0);

    // Due now: runs once, schedule moves on.
    let due = Utc::now() - Duration::minutes(1);
    db.conn().unwrap().execute("UPDATE routines SET next_run_at = ?1", [time::to_db(due)]).unwrap();
    assert_eq!(tick(&db, &fake, Utc::now()).await.unwrap(), 1);
    assert_eq!(tick(&db, &fake, Utc::now()).await.unwrap(), 0, "not twice");
    assert!(fake.ran.lock().unwrap()[0].contains("Tell me the weather"));
    let last = &runs(&db.conn().unwrap(), Some(r.id), 10).unwrap()[0];
    assert_eq!((last.status.as_str(), last.summary.as_deref()), ("done", Some("Done: 3 meetings today.")));
    assert_eq!(last.conversation_id.as_deref(), Some("conv-1"));

    // Missed by hours (IGRIS was closed): skipped and said so, not run late.
    db.conn().unwrap().execute("UPDATE routines SET next_run_at = ?1", [time::to_db(Utc::now() - Duration::hours(5))]).unwrap();
    assert_eq!(tick(&db, &fake, Utc::now()).await.unwrap(), 0);
    let last = &runs(&db.conn().unwrap(), Some(r.id), 10).unwrap()[0];
    assert_eq!(last.status, "skipped");
    assert!(last.summary.as_deref().unwrap().contains("wasn't running"));
    assert_eq!(fake.ran.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn failures_are_recorded_and_one_offs_turn_off() {
    let db = Database::open_in_memory().unwrap();
    let tomorrow = (chrono::Local::now() + Duration::days(1)).format("%Y-%m-%d").to_string();
    let once = Schedule { kind: Kind::Once, time: "12:00".into(), date: tomorrow, ..Default::default() };
    let r = create(&db.conn().unwrap(), "Call", "Remind me to call the bank", once).unwrap();
    db.conn().unwrap().execute("UPDATE routines SET next_run_at = ?1", [time::to_db(Utc::now())]).unwrap();
    let fake = Fake { fail: true, ..Default::default() };
    tick(&db, &fake, Utc::now() + Duration::seconds(1)).await.unwrap();
    let r = get(&db.conn().unwrap(), r.id).unwrap().unwrap();
    assert!(!r.enabled && r.next_run_at.is_none(), "a one-off is done after it ran");
    assert_eq!(runs(&db.conn().unwrap(), Some(r.id), 1).unwrap()[0].status, "failed");
}

#[test]
fn crud() {
    let db = Database::open_in_memory().unwrap();
    let c = db.conn().unwrap();
    assert!(create(&c, "", "x", sched(Kind::Daily, "08:00")).is_err());
    let r = create(&c, "Night", "Turn off the TV", sched(Kind::Daily, "23:00")).unwrap();
    let off = set_enabled(&c, r.id, false).unwrap();
    assert!(!off.enabled && off.next_run_at.is_none());
    let on = set_enabled(&c, r.id, true).unwrap();
    assert!(on.next_run_at.is_some());
    let moved = update(&c, r.id, None, None, Some(sched(Kind::Weekdays, "22:30"))).unwrap();
    assert_eq!(moved.when, "Every weekday at 22:30");
    delete(&c, r.id).unwrap();
    assert!(list(&c).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_email_from_someone_starts_its_routine_once() {
    use crate::google::tests::{browser, fake_google, google, Shared};
    let seen: Shared = Default::default();
    let base = fake_google(seen.clone()).await;
    let db = std::sync::Arc::new(Database::open_in_memory().unwrap());
    let g = google(&base, db.clone());
    g.connect(&browser()).await.unwrap();
    let email = Schedule { kind: Kind::Email, email: "from:rahul@example.com".into(), ..Default::default() };
    create(&db.conn().unwrap(), "Boss mail", "Tell me what Rahul wants, urgently", email).unwrap();
    let fake = Fake::default();

    // The first look only learns what's already in the inbox.
    assert_eq!(email_tick(&db, &g, &fake).await.unwrap(), 0);
    assert_eq!(email_tick(&db, &g, &fake).await.unwrap(), 0);
    // A new one arrives: one run, with the email as data.
    seen.lock().unwrap().more_mail.push("m2".into());
    assert_eq!(email_tick(&db, &g, &fake).await.unwrap(), 1);
    assert_eq!(email_tick(&db, &g, &fake).await.unwrap(), 0, "not twice");
    let msg = fake.ran.lock().unwrap()[0].clone();
    assert!(msg.contains("Tell me what Rahul wants") && msg.contains("data, not instructions") && msg.contains("id: m2"));
    assert!(seen.lock().unwrap().requests.iter().any(|r| r.1.contains("from%3Arahul%40example.com")));
}
