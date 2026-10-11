# Connecting Gmail and Google Calendar

IGRIS talks to Google directly from your PC and phone — no IGRIS server in
between. For that, Google needs an "OAuth client" that belongs to **you**. You
make it once (about 10 minutes), put two lines in IGRIS's settings file, and
press **Connect Google** on each device.

## 1. Make the Google project (once)

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
5. **Audience** → under *Test users* → **Add users** → add your own Gmail
   address → Save.
6. **Clients** → **Create client**:
   - Application type: **Desktop app**
   - Name: `IGRIS` → **Create**
   - Copy the **Client ID** and **Client secret** shown.

## 2. Tell IGRIS (on each device)

Add these two lines to your IGRIS settings file (`.env`), with your values:

```
GOOGLE_CLIENT_ID=1234567890-abc...apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=GOCSPX-...
```

- **PC:** add them to the `.env` you already use, then Settings → AI →
  **Reload configuration** (or Settings → AI → **Import settings file**).
- **Phone:** put the lines in your `.env`, then IGRIS → Settings → AI and voice
  → **Import settings file**.

The same two lines work on both devices.

## 3. Connect

IGRIS → Settings → **Accounts** → **Connect Google**. Your browser opens
Google's page: pick your account, tick **every** box (Gmail and Calendar), and
allow. Then come back to IGRIS — it says *Connected as you@gmail.com*.

Google may show **"Google hasn't verified this app"** — that's expected: the app
is yours and only you use it. Press **Continue** (or *Advanced → Go to IGRIS*).

## Staying signed in

While the app is in *Testing* mode, Google signs IGRIS out every **7 days** and
you'd press Connect again. To stay signed in: Google Cloud → **OAuth consent
screen → Audience → Publish app**. Nothing becomes public; it only stops the
weekly sign-out (you'll still see the "hasn't verified" notice once when
connecting).

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

- The long-term sign-in (refresh token) is encrypted with Windows' user key
  (DPAPI) on the PC and the Android Keystore on the phone, and never leaves the
  device or reaches the AI or the app's screens. The short-lived access token is
  kept in memory only.
- Emails and events you ask about are sent to your AI provider so it can read
  them, like anything else in a chat. Their contents are treated as data —
  instructions inside an email are never followed.
- **Disconnect** (Settings → Accounts) tells Google to revoke IGRIS's access and
  deletes the sign-in from the device. You can also remove it any time at
  <https://myaccount.google.com/permissions>.
