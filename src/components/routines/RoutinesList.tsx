import { listen } from "@tauri-apps/api/event";
import { CircleCheck, CircleX, Clock, Loader2, Play, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import type { Routine, RoutineRun } from "@/types/routines";
import { formatWhen } from "@/utils/time";

const EXAMPLES = [
  "“Every morning at 8, tell me the weather, my meetings and important unread emails.”",
  "“Every night at 11:30, turn off the TV and pause the music.”",
  "“When Rahul emails me, tell me right away what he wants.”",
  "“Every Sunday at 6 pm, plan my week from my calendar and to-dos.”",
];

function RunStatus({ run, phone }: { run: RoutineRun | null; phone: boolean }) {
  if (!run) return <span className={phone ? "m-muted" : "text-faint"}>Not run yet</span>;
  const icon =
    run.status === "running" ? (
      <Loader2 className="size-3.5 animate-spin" />
    ) : run.status === "done" ? (
      <CircleCheck className="size-3.5 text-success" />
    ) : run.status === "skipped" ? (
      <Clock className="size-3.5" />
    ) : (
      <CircleX className="size-3.5 text-danger" />
    );
  return (
    <span className="flex min-w-0 items-start gap-1.5">
      <span className="mt-0.5 shrink-0">{icon}</span>
      <span className="min-w-0 break-words">
        {formatWhen(run.startedAt)}
        {run.summary ? ` — ${run.summary}` : ""}
      </span>
    </span>
  );
}

/**
 * The user's routines: what, when, how the last run went; pause, run now,
 * delete. New ones are made by asking IGRIS.
 */
export function RoutinesList({ phone = false }: { phone?: boolean }) {
  const [items, setItems] = useState<Routine[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<number | null>(null);

  const load = useCallback(() => {
    api
      .routinesList()
      .then((r) => {
        setItems(r);
        setError(null);
      })
      .catch((err) => setError(BackendError.from(err).message));
  }, []);

  useEffect(() => {
    load();
    if (!hasBackend()) return;
    let off: (() => void) | undefined;
    let live = true;
    void listen("routines-changed", () => load()).then((u) => (live ? (off = u) : u()));
    const t = setInterval(load, 15000);
    return () => {
      live = false;
      off?.();
      clearInterval(t);
    };
  }, [load]);

  const act = (p: Promise<unknown>) =>
    void p.then(load, (err) => setError(BackendError.from(err).message));

  const muted = phone ? "m-muted text-sm" : "text-xs text-muted";
  const chip = phone ? "m-chip" : "flex items-center gap-1 rounded-full bg-surface-strong px-3 py-1.5 text-xs font-medium text-fg hover:bg-surface-hover";

  if (error) return <p className="text-sm text-danger">{error}</p>;
  if (!items) return null;

  return (
    <div className="space-y-3">
      {items.length === 0 ? (
        <div className={phone ? "m-card space-y-2 px-4 py-4" : "space-y-2"}>
          <p className={phone ? "text-[15px] font-medium" : "text-sm font-medium text-fg"}>No routines yet. Just ask IGRIS, for example:</p>
          <ul className={`space-y-1.5 ${muted}`}>
            {EXAMPLES.map((e) => (
              <li key={e}>{e}</li>
            ))}
          </ul>
        </div>
      ) : (
        <ul className="space-y-2" aria-label="Routines">
          {items.map((r) => (
            <li
              key={r.id}
              className={`rounded-2xl px-4 py-3 ${phone ? "" : "bg-surface"} ${r.enabled ? "" : "opacity-60"}`}
              style={phone ? { background: "var(--m-surface)" } : undefined}
            >
              <div className="flex items-start gap-3">
                <div className="min-w-0 flex-1">
                  <p className={phone ? "text-[16px] font-semibold" : "text-sm font-semibold text-fg"}>{r.name}</p>
                  <p className={muted}>
                    {r.when}
                    {r.enabled ? "" : " · paused"}
                  </p>
                  <p className={`mt-1 break-words ${phone ? "text-[15px]" : "text-sm text-fg"}`}>{r.instruction}</p>
                  <div className={`mt-1.5 ${muted}`}>
                    <RunStatus run={r.lastRun} phone={phone} />
                  </div>
                </div>
                <label className="flex shrink-0 items-center" title={r.enabled ? "On" : "Paused"}>
                  <input
                    type="checkbox"
                    role="switch"
                    aria-label={`${r.name} on`}
                    checked={r.enabled}
                    onChange={(e) => act(api.routineSetEnabled(r.id, e.target.checked))}
                    className="size-5 accent-[var(--accent,#5eb8ff)]"
                  />
                </label>
              </div>
              <div className="mt-2.5 flex flex-wrap gap-2">
                <button type="button" className={chip} onClick={() => act(api.routineRunNow(r.id))}>
                  <Play className="size-3.5" /> Run now
                </button>
                {confirm === r.id ? (
                  <button type="button" className={chip} onClick={() => act(api.routineDelete(r.id).then(() => setConfirm(null)))}>
                    <Trash2 className="size-3.5" /> Delete “{r.name}”?
                  </button>
                ) : (
                  <button type="button" className={chip} onClick={() => setConfirm(r.id)}>
                    <Trash2 className="size-3.5" /> Delete
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
      <p className={muted}>
        Routines run while IGRIS is running on this device (keep it open on your PC for daily ones). Anything that needs your OK asks with a
        notification — tap <b>Always allow</b> once and that routine won’t ask again for it. Sending, paying and deleting always ask.
      </p>
    </div>
  );
}
