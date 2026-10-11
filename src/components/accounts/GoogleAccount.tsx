import { openUrl } from "@tauri-apps/plugin-opener";
import { Calendar, Mail } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { GoogleStatus } from "@/types/google";

const GUIDE = "https://github.com/pragunanand14/I.G.R.I.S/blob/main/docs/GOOGLE.md";

/**
 * Connect Gmail and Google Calendar. Shows only whether it's set up and the
 * account's email; tokens never reach this screen. Phone and desktop share it.
 */
export function GoogleAccount({ phone = false }: { phone?: boolean }) {
  const [status, setStatus] = useState<GoogleStatus | null>(null);
  const [busy, setBusy] = useState<"connect" | "disconnect" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .googleStatus()
      .then(setStatus)
      .catch((err) => setError(BackendError.from(err).message));
  }, []);

  const run = (kind: "connect" | "disconnect", f: () => Promise<GoogleStatus>) => {
    setBusy(kind);
    setError(null);
    f()
      .then(setStatus)
      .catch((err) => setError(BackendError.from(err).message))
      .finally(() => setBusy(null));
  };

  const muted = phone ? "m-muted text-[15px]" : "text-sm text-muted";
  const primary = phone ? "m-button m-button-primary w-full" : "rounded-full bg-accent px-4 py-2 text-sm font-semibold text-bg hover:opacity-90 disabled:opacity-40";
  const secondary = phone ? "m-button w-full" : "rounded-full bg-surface-strong px-4 py-2 text-sm font-medium text-fg hover:bg-surface-hover disabled:opacity-40";

  if (!status) return error ? <p className="text-sm text-danger">{error}</p> : null;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <span className="flex gap-1 text-accent" aria-hidden="true">
          <Mail className="size-5" />
          <Calendar className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          <p className={phone ? "text-[15px] font-medium" : "text-sm font-medium text-fg"}>Gmail and Google Calendar</p>
          <p className={phone ? "m-muted text-sm" : "text-xs text-muted"}>
            {status.connected ? `Connected as ${status.email}` : status.configured ? "Not connected" : "Needs a one-time setup"}
          </p>
        </div>
      </div>

      {status.connected ? (
        <>
          <p className={muted}>
            Ask things like “any important emails?”, “reply to Rahul that 5 works”, “what’s on tomorrow?”, “move my 4 pm to Friday”. Sending an email,
            deleting and inviting people always ask you first. Meetings get a reminder 10 minutes before.
          </p>
          <button type="button" className={secondary} disabled={busy !== null} onClick={() => run("disconnect", api.googleDisconnect)}>
            {busy === "disconnect" ? "Disconnecting…" : "Disconnect"}
          </button>
        </>
      ) : status.configured ? (
        <>
          <p className={muted}>Google opens in your browser. Sign in, tick every box, and come back here.</p>
          <button type="button" className={primary} disabled={busy !== null} onClick={() => run("connect", api.googleConnect)}>
            {busy === "connect" ? "Waiting for Google… (finish in your browser)" : "Connect Google"}
          </button>
        </>
      ) : (
        <p className={muted}>
          First, a 10-minute one-time setup on Google’s site: follow the{" "}
          <button
            type="button"
            onClick={() => void openUrl(GUIDE).catch(() => setError(`Open this link in your browser: ${GUIDE}`))}
            className={phone ? "m-link" : "text-accent underline-offset-2 hover:underline"}
          >
            Google setup guide
          </button>
          , then add the two lines it gives you to IGRIS’s settings file (Settings → AI → Import settings file) and come back here.
        </p>
      )}
      {error && <p className={phone ? "px-1 text-sm" : "text-xs text-danger"} style={phone ? { color: "var(--m-bad)" } : undefined}>{error}</p>}
    </div>
  );
}
