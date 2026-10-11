# Errands on autopilot

What IGRIS can do end to end, and where it always stops for you.

## Web errands (computer)

Ordering, booking, renewing, recharging, filling in an application: IGRIS
works in **its own browser window** (operator mode, with the border and orb
showing it's in control). It fills in what it already knows — your memory,
the people you've told it about, saved places — and asks only for what it
can't know.

It **stops at the final step**: paying, placing the order or submitting is a
`browser_confirmed_click`, which always asks you, showing the button actually
on the page and what will happen (e.g. "Pay ₹1,299 to Amazon for 2 items").
"Always allow" is never offered for these.

**IGRIS never types card numbers, CVV, UPI PINs, OTPs or passwords.** The
browser, computer and phone typing tools refuse text that is a card number
(Luhn check) and fields labelled password, PIN, CVV/CVC, card number or
one-time code (an Indian "PIN code" for addresses is fine). IGRIS asks you to
type those in the window yourself, then carries on.

## Sending documents

"Email my resume to hr@company.com": IGRIS finds the file in your **shared
folders** (Tools → Shared folders) and attaches it to a Gmail draft
(`gmail_draft` with `attach`). Sending still asks you, and the prompt lists the
attachments. Limits: files only from shared folders; credential and key files
(.env, .pem, …) are never attached; up to 5 MB together (bigger files are
better shared as a Google Drive link).

## Tidy Downloads (computer)

Tools → Tidy Downloads, or ask "tidy my downloads". Loose files in Downloads
go into Documents, Images, Videos, Music, Archives and Installers folders.
Folders, hidden files, downloads still in progress (.crdownload, .part, …)
and files changed in the last 3 minutes stay; other file types stay where
they are. Nothing is renamed over or overwritten (a taken name gets " (2)").
**Undo last tidy** puts the files back. "Tidy by itself" checks every 10
minutes while IGRIS is running. Asking IGRIS to tidy always asks first
(you can "Always allow" it).

## On the phone

Phone control (Settings → Screen control) can work through apps the same way
(open the app, fill in, stop before paying/sending/deleting — those always
ask), with the same never-type-secrets rule.
