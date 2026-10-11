# Routines

Routines are things IGRIS does by itself. You make one by asking:

- "Every morning at 8, tell me the weather, my meetings and important unread emails."
- "Every night at 11:30, turn off the TV and pause the music."
- "When Rahul emails me, tell me right away what he wants."
- "Every Sunday at 6 pm, plan my week from my calendar and to-dos."
- "On 2026-12-24 at 9 am, remind me to buy gifts and list what I still need."

IGRIS turns that into a **trigger** (every day / weekdays / weekends / chosen
days / once at a time, or *when an email matching a Gmail search arrives*) and
an **instruction** written to make sense on its own. See, pause, run or delete
them on the **Routines** page (PC: top bar; phone: Settings → Routines), or ask.

## How a run works

Each run is an ordinary conversation ("Routine "Morning": …") with the same
tools, permission checks and audit log as a chat. When it finishes, its reply
arrives as a notification and the run is listed with its outcome.

If an action needs your OK, you get a notification and an approval card on any
IGRIS screen: **Approve**, **Deny**, or **Always allow** — after which that
routine (and any chat) won't ask again for that kind of action. Sending email,
paying, deleting and similar always ask, by design. An approval nobody answers
within 5 minutes is denied, and the run says so.

## Limits (honest ones)

- Routines run while IGRIS is **running on that device**. For daily ones, keep
  IGRIS open on your PC. On the phone, Android may stop IGRIS in the background,
  so phone routines are less reliable.
- A run that was due while IGRIS was closed is **skipped** if it's more than
  two hours late (a 7 am briefing at 3 pm helps nobody); the Routines page says
  so. One-off routines turn themselves off after running.
- Email triggers need Gmail connected (Settings → Accounts) and check every few
  minutes; the first check only learns what's already in your inbox.
- Email contents that start a routine are passed to it as data, never as
  instructions.

## The daily briefing

Routines page → **Daily briefing**: pick a time (every day or weekdays), what
to include — weather, meetings, important emails, to-dos and reminders — news
topics (needs web search) and your city. IGRIS makes it a routine called
"Daily briefing". Each morning its text arrives as a notification, and Home
shows **Today's briefing** with **Listen** (read aloud with your IGRIS voice).

Weather comes from Open-Meteo (free, no account): only your chosen city's
coordinates are sent, never your location. The same city is used when you ask
"what's the weather?" and, on the phone, by the weather widget.
