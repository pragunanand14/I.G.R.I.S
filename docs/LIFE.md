# Life: people and spending

IGRIS keeps two small, local records so it can help without asking every time.
Both live only in IGRIS's database on the device (they are not synced between
devices) and can be seen, changed and deleted on the **Life** page (computer)
or **Settings → Life** (phone).

## People

Who someone is to you, their birthday and a few notes ("loves orchids", "gift
ideas: books"). IGRIS adds people when you tell it about them in chat
(`save_person`), looks them up when you mention them (`find_people`), and the
daily briefing mentions birthdays in the next few days. Notes that look like
passwords, card or ID numbers are refused, as in Memory.

## Spending

Payments are found in:

- **Payment messages on the phone**: bank/UPI SMS and card alerts, from the
  message notifications IGRIS can already see (Settings → Messages). Android
  keeps these for about a day, so turn on **Track by itself** (it checks every
  few hours while IGRIS is running) or ask regularly.
- **Payment emails in Gmail** (when Google is connected, on any device): the
  subject and preview of emails such as "Rs 499 debited", "You paid ₹120",
  "Payment successful".
- **What you tell IGRIS**: "I spent 300 on lunch" (`log_expense`).

The text is parsed **on the device** by fixed rules (`igris-core/src/expenses.rs`):
only the amount, the shop and the date are kept, and only those reach the AI
when you ask about your spending. A message is skipped unless it is clearly
money going out: credits, OTPs, bills that are only due, failed or reversed
payments and refunds are ignored. Each message counts once however often it is
scanned, and a payment seen in both an SMS and an email on the same day (same
amount) counts once.

Categories (Food, Groceries, Transport, Shopping, Bills, Entertainment, Health,
Transfers, Other) are a best guess from the shop's name; change any of them on
the page or by asking. A payment that wasn't really spending (for example
moving money to your own account) can be removed.

Limits, honestly: the rules know common Indian bank/UPI and card-alert
wordings in ₹, $, € and £; an unusual bank format may be missed (tell IGRIS
the amount instead). Message notifications only cover the time IGRIS could
see them; IGRIS does not read the SMS inbox.
