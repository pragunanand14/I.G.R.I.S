# Connecting Gmail and Google Calendar

IGRIS talks to Google directly from your PC and phone — no IGRIS server in
between. Google only shows its sign-in page to apps it knows, so IGRIS has to be
registered with Google once. Because IGRIS is your own app, that's done in your
own Google account: about 10 minutes, once. After that, every device just has a
**Sign in with Google** button.

- **Phone:** signs in with the Google account already on the phone (Google's own
  account picker). Nothing to paste.
- **PC:** paste two values from step 1 into IGRIS once.

## 1. Register IGRIS with Google (once, on a computer)

1. Open <https://console.cloud.google.com> and sign in with the Google account
   you want IGRIS to use.
2. Top bar → project picker → **New project** → name it `IGRIS` → **Create**.
   Make sure it's selected.
3. Menu → **APIs & Services** → **Library**. Search and **Enable** both:
   - **Gmail API**
   - **Google Calendar API**
4. Menu → **APIs & Services** → **OAuth consent screen** (also called *Google
   Auth Platform*) → **Get started**:
   - App name: `IGRIS`, support email: yours → Next
   - Audience: **External** → Next
   - Contact email: yours → Next → agree → **Create**
5. **Audience** → **Publish app** → Confirm. (This doesn't make anything public.
   Without it Google signs IGRIS out every 7 days.)
6. **Clients** → **Create client** — the PC's entry:
   - Application type: **Desktop app**, name: `IGRIS PC` → **Create**
   - Keep the **Client ID** and **Client secret** it shows (for step 2).
7. **Clients** → **Create client** again — the phone's entry:
   - Application type: **Android**, name: `IGRIS phone`
   - **Package name** and **SHA-1 certificate fingerprint**: on the phone, open
     IGRIS → Settings → Accounts → *First time? Google needs to know this copy of
     IGRIS* — it shows both, with Copy buttons (send them to yourself, or type
     them in).
   - **Create**. (Nothing to copy from this one.)

## 2. Connect

- **Phone:** IGRIS → Settings → **Accounts** → **Sign in with Google** → pick
  your account → allow.
- **PC:** IGRIS → Settings → **Accounts** → paste the **Client ID** and
  **Client secret** from step 1.6 → **Save** → **Sign in with Google**. Your
  browser opens Google's page: pick your account, tick **every** box, allow.
  (Instead of pasting, `GOOGLE_CLIENT_ID=` / `GOOGLE_CLIENT_SECRET=` lines in
  IGRIS's `.env` still work.)

Google shows **"Google hasn't verified this app"** the first time — expected:
the app is yours and only you use it. Tap **Advanced → Go to IGRIS (unsafe)**.

If the phone says *Google doesn't recognise this copy of IGRIS*, step 1.7 is
missing or its fingerprint doesn't match. The fingerprint changes if the phone
app is ever signed with a different key (e.g. a fresh install from a different
build): add another Android entry with the new one.

## What IGRIS can do, and what always asks

| | |
|---|---|
| Search and read email, summarise, find things | runs (it's your data, read on request) |
| Draft emails and replies | runs — drafts are saved in your Gmail, nothing is sent |
| **Send** an email | **always asks**, showing recipient, subject and text |
| Archive, mark read, star | runs (can be *Always allow*ed per action) |
| **Move to Bin** | **always asks** |
| See your calendar, add or move your own events | runs |
| Add an event **with invitees** (Google emails them) | **always asks** |
| **Delete** an event | **always asks** |
| Reminder 10 minutes before each meeting | automatic while connected (rings on the phone even when IGRIS is closed) |

## Privacy and security

- On the phone, Google Play services keeps the sign-in; IGRIS only asks it for
  a short-lived access token when it needs one. On the PC, the long-term
  sign-in (refresh token) and the pasted client are encrypted with Windows'
  user key (DPAPI), and never leaves the
  device or reaches the AI or the app's screens. The short-lived access token is
  kept in memory only.
- Emails and events you ask about are sent to your AI provider so it can read
  them, like anything else in a chat. Their contents are treated as data —
  instructions inside an email are never followed.
- **Disconnect** (Settings → Accounts) deletes the sign-in from the device; on
  the PC it also tells Google to revoke IGRIS's access. (On the phone the grant
  stays with the phone's Google account until you remove it at the link below.) You can also remove it any time at
  <https://myaccount.google.com/permissions>.
