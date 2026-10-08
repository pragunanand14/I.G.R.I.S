import { CheckCircle2, CircleSlash, Clock, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import type { AuditEntry } from "@/types/tools";
import { outcome } from "../activity";
import { whenText } from "../format";
import { ErrorText, Screen } from "../ui";

/** Everything IGRIS has done with its tools, newest first. */
export function ActivityScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [items, setItems] = useState<AuditEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!ready) return;
    api
      .listToolAudit(100)
      .then(setItems)
      .catch((err) => setError(BackendError.from(err).message));
  }, [ready]);

  return (
    <Screen title="Activity" subtitle="Every action IGRIS has taken, and whether you approved it." back="/more">
      {error && <ErrorText>{error}</ErrorText>}
      {!ready ? (
        <p className="m-muted">Activity appears here once IGRIS is running.</p>
      ) : (
        <ul className="m-group" aria-label="Activity">
          {items?.length === 0 && <li className="m-row m-muted">Nothing yet.</li>}
          {items?.map((e) => {
            const o = outcome(e);
            const Icon = o.tone === "ok" ? CheckCircle2 : o.tone === "bad" ? XCircle : e.approval === "denied" ? CircleSlash : Clock;
            const color = o.tone === "ok" ? "var(--m-ok)" : o.tone === "bad" ? "var(--m-bad)" : "var(--m-faint)";
            return (
              <li key={e.id} className="m-row items-start">
                <Icon className="mt-0.5 size-[18px] shrink-0" style={{ color }} />
                <div className="min-w-0 flex-1">
                  <p className="break-words">{e.description}</p>
                  <p className="m-faint mt-0.5 text-sm">
                    {o.text} · {whenText(e.createdAt)}
                  </p>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </Screen>
  );
}
