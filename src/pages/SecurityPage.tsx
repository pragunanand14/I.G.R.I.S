import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { PermissionBadge } from "@/components/ui/PermissionBadge";
import { Toggle } from "@/components/ui/Toggle";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { PERMISSION_INFO, type AuditEntry, type PermissionRule } from "@/types/tools";

const STATUS_TONE: Record<string, string> = {
  completed: "text-success",
  failed: "text-danger",
  invalid: "text-danger",
  denied: "text-warning",
  cancelled: "text-muted",
};

const GUARANTEES = [
  "IGRIS can only act through the registered tools listed on the Tools page — there is no shell or arbitrary command execution.",
  "Every tool input is validated against its schema before anything runs; unknown tools are rejected.",
  "Applications can only be opened from your allowlist, by name. Paths and arguments never come from the model.",
  "Sensitive and critical actions always wait for your explicit approval and expire after 5 minutes without an answer.",
  "API keys are read only by the native backend and are never sent to this interface or written to logs.",
  "Tool results are treated as data, not instructions.",
];

export function SecurityPage() {
  const backend = useAppStore((s) => s.backend);
  const { settings, status, saving, update } = useSettingsStore();
  const [rules, setRules] = useState<PermissionRule[]>([]);
  const [log, setLog] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const refresh = () => {
    setLoading(true);
    setReloadKey((k) => k + 1);
  };

  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    Promise.all([api.getPermissionPolicy(), api.listToolAudit(200)])
      .then(([r, l]) => {
        if (!live) return;
        setRules(r);
        setLog(l);
        setError(null);
      })
      .catch((err) => live && setError(BackendError.from(err).message))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [backend, reloadKey, settings.confirmLowRisk]);

  if (backend !== "ready") {
    return (
      <div className="p-8">
        <PageHeader title="Security" />
        <p className="text-sm text-danger">Security settings require the IGRIS desktop backend.</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-5xl p-8">
      <PageHeader title="Security" description="What IGRIS is allowed to do, and a record of everything it did." />
      {error && <p className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{error}</p>}

      <div className="space-y-4">
        <Panel title="Permission policy">
          <ul className="divide-y divide-line">
            {rules.map((r) => (
              <li key={r.level} className="flex items-center gap-4 py-3 first:pt-0 last:pb-0">
                <div className="w-20 shrink-0">
                  <PermissionBadge level={r.level} />
                </div>
                <p className="min-w-0 flex-1 text-sm text-muted">{PERMISSION_INFO[r.level].summary}</p>
                {r.configurable ? (
                  <label className="flex shrink-0 items-center gap-2 text-xs text-muted">
                    Ask first
                    <Toggle
                      label="Ask before low-risk actions"
                      checked={settings.confirmLowRisk}
                      disabled={status !== "ready" || saving}
                      onChange={(confirmLowRisk) => void update({ confirmLowRisk })}
                    />
                  </label>
                ) : (
                  <span className={`shrink-0 text-xs ${r.requiresApproval ? "text-warning" : "text-faint"}`}>
                    {r.requiresApproval ? "Always asks" : "Never asks"}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </Panel>

        <Panel title="Guarantees">
          <ul className="space-y-1.5 text-sm text-muted">
            {GUARANTEES.map((g) => (
              <li key={g} className="flex gap-2">
                <span className="text-accent">—</span>
                {g}
              </li>
            ))}
          </ul>
        </Panel>

        <Panel
          title="Audit log"
          action={
            <button
              type="button"
              onClick={refresh}
              disabled={loading}
              className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40"
            >
              <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} /> Refresh
            </button>
          }
        >
          {log.length === 0 ? (
            <p className="text-sm text-faint">No tool activity yet.</p>
          ) : (
            <div className="-mx-4 overflow-x-auto">
              <table className="w-full text-left text-xs">
                <thead className="text-label">
                  <tr className="border-b border-line">
                    <th className="px-4 py-2 font-medium">Time</th>
                    <th className="px-2 py-2 font-medium">By</th>
                    <th className="px-2 py-2 font-medium">Action</th>
                    <th className="px-2 py-2 font-medium">Level</th>
                    <th className="px-2 py-2 font-medium">Approval</th>
                    <th className="px-2 py-2 font-medium">Outcome</th>
                    <th className="px-4 py-2 text-right font-medium">ms</th>
                  </tr>
                </thead>
                <tbody>
                  {log.map((e) => (
                    <tr key={e.id} className="border-b border-line last:border-0 hover:bg-surface-hover">
                      <td className="px-4 py-2 whitespace-nowrap text-faint tabular">{new Date(e.createdAt).toLocaleString([], { dateStyle: "short", timeStyle: "medium" })}</td>
                      <td className="px-2 py-2 text-muted">{e.actor === "user" ? "You" : "IGRIS"}</td>
                      <td className="max-w-64 px-2 py-2" data-selectable>
                        <div className="truncate text-fg" title={e.description}>
                          {e.description}
                        </div>
                        {e.result && (
                          <div className="truncate text-faint" title={e.result}>
                            {e.result}
                          </div>
                        )}
                      </td>
                      <td className="px-2 py-2">
                        {e.permission === "unknown" ? <span className="text-faint">—</span> : <PermissionBadge level={e.permission} />}
                      </td>
                      <td className="px-2 py-2 text-muted">{e.approval}</td>
                      <td className={`px-2 py-2 ${STATUS_TONE[e.status] ?? "text-muted"}`}>{e.status}</td>
                      <td className="px-4 py-2 text-right font-mono text-faint">{e.durationMs ?? "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Panel>
      </div>
    </div>
  );
}
