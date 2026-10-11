import { listen } from "@tauri-apps/api/event";
import { RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import {
  CATEGORIES,
  money,
  type SpendingPeriod,
  type SpendingView,
} from "@/types/life";
import { lifeStyles } from "./styles";

const PERIODS: { value: SpendingPeriod; label: string }[] = [
  { value: "this_week", label: "This week" },
  { value: "this_month", label: "This month" },
  { value: "last_month", label: "Last month" },
  { value: "this_year", label: "This year" },
];

const SOURCE: Record<string, string> = {
  phone: "message",
  email: "email",
  manual: "you",
};

/**
 * What you spent: found in payment messages (phone) and payment emails
 * (Gmail), or told to IGRIS. Totals by category, each payment fixable.
 */
export function Spending({ phone = false }: { phone?: boolean }) {
  const s = lifeStyles(phone);
  const [period, setPeriod] = useState<SpendingPeriod>("this_month");
  const [view, setView] = useState<SpendingView | null>(null);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api
      .spendingView(period)
      .then(setView, (err) =>
        setNote({ ok: false, text: BackendError.from(err).message }),
      );
  }, [period]);

  useEffect(() => {
    load();
    if (!hasBackend()) return;
    let off: (() => void) | undefined;
    let live = true;
    void listen("expenses-changed", () => load()).then((u) =>
      live ? (off = u) : u(),
    );
    return () => {
      live = false;
      off?.();
    };
  }, [load]);

  const fail = (err: unknown) =>
    setNote({ ok: false, text: BackendError.from(err).message });

  const scan = async () => {
    setBusy(true);
    setNote(null);
    try {
      const r = await api.expensesScan();
      setNote({
        ok: true,
        text: [
          `Found ${r.added} new payment${r.added === 1 ? "" : "s"}.`,
          ...r.notes,
        ].join(" "),
      });
      load();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  if (!view)
    return note ? <p className="text-sm text-danger">{note.text}</p> : null;
  const currencies = [...new Set(view.totals.map((t) => t.currency))];
  const canScan = view.phone || view.email;

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-2" role="group" aria-label="Period">
        {PERIODS.map((p) => (
          <button
            key={p.value}
            type="button"
            aria-pressed={period === p.value}
            className={`${s.chip} ${period === p.value ? (phone ? "m-accent" : "bg-accent! text-bg!") : ""}`}
            onClick={() => setPeriod(p.value)}
          >
            {p.label}
          </button>
        ))}
      </div>

      <div className={s.card}>
        {currencies.length === 0 ? (
          <p className={s.text}>Nothing recorded for this period yet.</p>
        ) : (
          currencies.map((cur) => {
            const rows = view.totals.filter((t) => t.currency === cur);
            const total = rows.reduce((a, t) => a + t.amountMinor, 0);
            return (
              <div key={cur} className="space-y-2">
                <p
                  className={
                    phone
                      ? "text-[22px] font-semibold"
                      : "text-xl font-semibold text-fg"
                  }
                >
                  {money(total, cur)}
                </p>
                <ul
                  className="space-y-1.5"
                  aria-label={`Spending by category (${cur})`}
                >
                  {rows.map((t) => (
                    <li key={t.category} className="space-y-0.5">
                      <div className={`flex justify-between gap-3 ${s.text}`}>
                        <span>
                          {t.category}{" "}
                          <span className={s.muted}>· {t.count}</span>
                        </span>
                        <span>{money(t.amountMinor, cur)}</span>
                      </div>
                      <div className="h-1.5 overflow-hidden rounded-full bg-[color-mix(in_srgb,currentColor_12%,transparent)]">
                        <div
                          className="h-full rounded-full bg-accent"
                          style={{
                            width: `${Math.max(2, (t.amountMinor / total) * 100)}%`,
                          }}
                        />
                      </div>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })
        )}
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className={s.chip}
            disabled={busy || !canScan}
            onClick={() => void scan()}
          >
            <RefreshCw className={`size-3.5 ${busy ? "animate-spin" : ""}`} />{" "}
            {busy ? "Looking…" : "Look for payments now"}
          </button>
          <label className={`flex items-center gap-2 ${s.text}`}>
            <input
              type="checkbox"
              role="switch"
              checked={view.auto}
              disabled={!canScan}
              onChange={(e) =>
                void api.expensesSetAuto(e.target.checked).then(load, fail)
              }
            />
            Track by itself
          </label>
        </div>
        <p className={s.muted}>
          {canScan
            ? `IGRIS reads ${[view.phone && "payment messages on this phone", view.email && "payment emails in Gmail"].filter(Boolean).join(" and ")} here on this device and keeps only the amount, the shop and the date. With “Track by itself” it checks every few hours while IGRIS is running.`
            : "Connect Google (Settings → Accounts) for payment emails, or use IGRIS on your phone for payment messages. You can also just tell IGRIS: “I spent 300 on lunch”."}
        </p>
        {note && (
          <p className={`${s.muted} ${note.ok ? "" : "text-danger"}`}>
            {note.text}
          </p>
        )}
      </div>

      {view.items.length > 0 && (
        <ul className="space-y-2" aria-label="Payments">
          {view.items.map((e) => (
            <li
              key={e.id}
              className={
                phone ? "m-card px-4 py-3" : "rounded-2xl bg-surface px-4 py-3"
              }
            >
              <div className="flex items-center gap-3">
                <div className="min-w-0 flex-1">
                  <p className={`truncate ${s.title}`}>
                    {money(e.amountMinor, e.currency)} ·{" "}
                    {e.merchant || "Unknown"}
                  </p>
                  <p className={s.muted}>
                    {e.spentOn} · from {SOURCE[e.source] ?? e.source}
                  </p>
                </div>
                <select
                  aria-label={`Category of ${e.merchant || "payment"}`}
                  value={e.category}
                  className={
                    phone
                      ? "m-input w-auto"
                      : "rounded-lg bg-surface-strong px-2 py-1 text-xs text-fg"
                  }
                  onChange={(ev) =>
                    void api
                      .expenseSetCategory(e.id, ev.target.value)
                      .then(load, fail)
                  }
                >
                  {CATEGORIES.map((c) => (
                    <option key={c}>{c}</option>
                  ))}
                </select>
                <button
                  type="button"
                  className={s.chip}
                  aria-label="Not an expense — remove"
                  onClick={() => void api.expenseDelete(e.id).then(load, fail)}
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
