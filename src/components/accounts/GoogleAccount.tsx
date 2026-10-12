import { openUrl } from "@tauri-apps/plugin-opener";
import { Calendar, Mail } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { GoogleSetupInfo, GoogleStatus } from "@/types/google";

const GUIDE =
  "https://github.com/pragunanand14/I.G.R.I.S/blob/main/docs/GOOGLE.md";

/**
 * Connect Gmail and Google Calendar. Shows only whether it's set up and the
 * account's email; tokens never reach this screen. Phone and desktop share it:
 * the phone signs in with the Google account already on it; a computer uses
 * the Client ID and secret from the setup guide, pasted here once.
 */
export function GoogleAccount({ phone = false }: { phone?: boolean }) {
  const [status, setStatus] = useState<GoogleStatus | null>(null);
  const [busy, setBusy] = useState<"connect" | "disconnect" | "save" | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [setup, setSetup] = useState<GoogleSetupInfo | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    api
      .googleStatus()
      .then(setStatus)
      .catch((err) => setError(BackendError.from(err).message));
  }, []);

  // The phone's sign-in needs Google to know this copy of IGRIS: show what to give it.
  const native = status?.native === true;
  const connected = status?.connected === true;
  useEffect(() => {
    if (native && !connected) {
      api
        .googleSetupInfo()
        .then(setSetup)
        .catch(() => setSetup(null));
    }
  }, [native, connected]);

  const run = (
    kind: "connect" | "disconnect" | "save",
    f: () => Promise<GoogleStatus>,
    after?: () => void,
  ) => {
    setBusy(kind);
    setError(null);
    f()
      .then((s) => {
        setStatus(s);
        after?.();
      })
      .catch((err) => setError(BackendError.from(err).message))
      .finally(() => setBusy(null));
  };

  const copy = (label: string, text: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => setCopied(label))
      .catch(() =>
        setError("Couldn't copy — select the text and copy it instead."),
      );
  };

  const muted = phone ? "m-muted text-[15px]" : "text-sm text-muted";
  const small = phone ? "m-muted text-sm" : "text-xs text-muted";
  const primary = phone
    ? "m-button m-button-primary w-full"
    : "rounded-full bg-accent px-4 py-2 text-sm font-semibold text-bg hover:opacity-90 disabled:opacity-40";
  const secondary = phone
    ? "m-button w-full"
    : "rounded-full bg-surface-strong px-4 py-2 text-sm font-medium text-fg hover:bg-surface-hover disabled:opacity-40";
  const link = phone
    ? "m-link"
    : "text-accent underline-offset-2 hover:underline";
  const input = phone
    ? "m-input w-full"
    : "w-full rounded-lg border border-border bg-surface px-3 py-2 text-sm text-fg placeholder:text-muted";

  const guide = (
    <button
      type="button"
      onClick={() =>
        void openUrl(GUIDE).catch(() =>
          setError(`Open this link in your browser: ${GUIDE}`),
        )
      }
      className={link}
    >
      Google setup guide
    </button>
  );

  if (!status)
    return error ? <p className="text-sm text-danger">{error}</p> : null;

  const clientForm = (
    <form
      className="space-y-2"
      onSubmit={(e) => {
        e.preventDefault();
        run(
          "save",
          () => api.googleSetClient(clientId, secret),
          () => {
            setEditing(false);
            setClientId("");
            setSecret("");
          },
        );
      }}
    >
      <input
        className={input}
        aria-label="Client ID"
        placeholder="Client ID (…apps.googleusercontent.com)"
        value={clientId}
        onChange={(e) => setClientId(e.target.value)}
        autoComplete="off"
        spellCheck={false}
      />
      <input
        className={input}
        aria-label="Client secret"
        placeholder="Client secret (GOCSPX-…)"
        type="password"
        value={secret}
        onChange={(e) => setSecret(e.target.value)}
        autoComplete="off"
        spellCheck={false}
      />
      <button
        type="submit"
        className={primary}
        disabled={busy !== null || !clientId.trim() || !secret.trim()}
      >
        {busy === "save" ? "Saving…" : "Save"}
      </button>
      <p className={small}>Kept encrypted on this computer only.</p>
    </form>
  );

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <span className="flex gap-1 text-accent" aria-hidden="true">
          <Mail className="size-5" />
          <Calendar className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          <p
            className={
              phone ? "text-[15px] font-medium" : "text-sm font-medium text-fg"
            }
          >
            Gmail and Google Calendar
          </p>
          <p className={small}>
            {status.connected
              ? `Connected as ${status.email}`
              : status.configured
                ? "Not connected"
                : "Needs a one-time setup"}
          </p>
        </div>
      </div>

      {status.connected ? (
        <>
          <p className={muted}>
            Ask things like “any important emails?”, “reply to Rahul that 5
            works”, “what’s on tomorrow?”, “move my 4 pm to Friday”. Sending an
            email, deleting and inviting people always ask you first. Meetings
            get a reminder 10 minutes before.
          </p>
          <button
            type="button"
            className={secondary}
            disabled={busy !== null}
            onClick={() => run("disconnect", api.googleDisconnect)}
          >
            {busy === "disconnect" ? "Disconnecting…" : "Disconnect"}
          </button>
        </>
      ) : native ? (
        <>
          <p className={muted}>
            Pick your Google account — the one already on this phone — and allow
            Gmail and Calendar.
          </p>
          <button
            type="button"
            className={primary}
            disabled={busy !== null}
            onClick={() => run("connect", api.googleConnect)}
          >
            {busy === "connect" ? "Waiting for Google…" : "Sign in with Google"}
          </button>
          <details className={small}>
            <summary>
              First time? Google needs to know this copy of IGRIS
            </summary>
            <div className="mt-2 space-y-2">
              <p>
                In the {guide}, the “Android” step asks for these two values.
                Copy them from here:
              </p>
              {setup ? (
                (
                  [
                    ["Package name", setup.packageName],
                    ["SHA-1 fingerprint", setup.sha1],
                  ] as const
                ).map(([label, value]) => (
                  <div key={label} className="flex items-center gap-2">
                    <div className="min-w-0 flex-1">
                      <p className="font-medium">{label}</p>
                      <p className="break-all font-mono">{value || "—"}</p>
                    </div>
                    {value && (
                      <button
                        type="button"
                        className={link}
                        onClick={() => copy(label, value)}
                      >
                        {copied === label ? "Copied" : "Copy"}
                      </button>
                    )}
                  </div>
                ))
              ) : (
                <p>Couldn’t read them from the phone.</p>
              )}
            </div>
          </details>
        </>
      ) : status.configured && !editing ? (
        <>
          <p className={muted}>
            Google opens in your browser. Sign in, tick every box, and come back
            here.
          </p>
          <button
            type="button"
            className={primary}
            disabled={busy !== null}
            onClick={() => run("connect", api.googleConnect)}
          >
            {busy === "connect"
              ? "Waiting for Google… (finish in your browser)"
              : "Sign in with Google"}
          </button>
          <button
            type="button"
            className={link}
            onClick={() => setEditing(true)}
          >
            Change the Google setup
          </button>
        </>
      ) : (
        <>
          <p className={muted}>
            One time only: follow the {guide} (about 10 minutes), then paste the
            two values it gives you here.
          </p>
          {clientForm}
          {editing && (
            <button
              type="button"
              className={link}
              onClick={() => setEditing(false)}
            >
              Cancel
            </button>
          )}
        </>
      )}
      {error && (
        <p
          className={phone ? "px-1 text-sm" : "text-xs text-danger"}
          style={phone ? { color: "var(--m-bad)" } : undefined}
        >
          {error}
        </p>
      )}
    </div>
  );
}
