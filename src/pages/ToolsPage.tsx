import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { AppWindow, FolderOpen, Play, Plus, ScanSearch, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { PermissionBadge } from "@/components/ui/PermissionBadge";
import { SharedFolders } from "@/components/tools/SharedFolders";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import type { AppCandidate, AppEntry, ToolInfo } from "@/types/tools";

type Notice = { ok: boolean; text: string } | null;

export function ToolsPage() {
  const backend = useAppStore((s) => s.backend);
  const platform = useAppStore((s) => s.info?.platform);
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [apps, setApps] = useState<AppEntry[]>([]);
  const [candidates, setCandidates] = useState<AppCandidate[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [path, setPath] = useState("");

  const report = (err: unknown) => setNotice({ ok: false, text: BackendError.from(err).message });
  const onFolderNotice = useCallback((n: { ok: boolean; text: string }) => setNotice(n), []);

  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    Promise.all([api.listTools(), api.listApplications()])
      .then(([t, a]) => {
        if (!live) return;
        setTools(t);
        setApps(a);
      })
      .catch((err) => live && setNotice({ ok: false, text: BackendError.from(err).message }));
    return () => {
      live = false;
    };
  }, [backend]);

  const add = async (n: string, p: string) => {
    try {
      const entry = await api.addApplication(n, p);
      setApps((list) => [...list, entry].sort((x, y) => x.name.localeCompare(y.name)));
      setCandidates((c) => c?.filter((x) => x.path !== p) ?? null);
      setNotice({ ok: true, text: `${entry.name} added. IGRIS can now open it.` });
      return true;
    } catch (err) {
      report(err);
      return false;
    }
  };

  const browse = async () => {
    try {
      const filters = platform === "windows" ? [{ name: "Applications", extensions: ["exe"] }] : undefined;
      const picked = await openFileDialog({ multiple: false, directory: false, filters, title: "Choose an application" });
      if (typeof picked === "string") {
        setPath(picked);
        if (!name.trim()) {
          const base = picked.split(/[\\/]/).pop() ?? "";
          setName(base.replace(/\.(exe|app)$/i, ""));
        }
      }
    } catch (err) {
      report(err);
    }
  };

  const scan = async () => {
    setScanning(true);
    try {
      setCandidates(await api.detectApplications());
    } catch (err) {
      report(err);
    } finally {
      setScanning(false);
    }
  };

  const launch = async (a: AppEntry) => {
    setBusyId(a.id);
    setNotice(null);
    try {
      const act = await api.launchApplication(a.id);
      setNotice({ ok: true, text: act.result ?? `${a.name} opened.` });
    } catch (err) {
      report(err);
    } finally {
      setBusyId(null);
    }
  };

  const remove = async (a: AppEntry) => {
    try {
      await api.removeApplication(a.id);
      setApps((list) => list.filter((x) => x.id !== a.id));
      setNotice({ ok: true, text: `${a.name} removed. IGRIS can no longer open it.` });
    } catch (err) {
      report(err);
    } finally {
      setConfirmRemove(null);
    }
  };

  if (backend !== "ready") {
    return (
      <div className="p-8">
        <PageHeader title="Tools" />
        <p className="text-sm text-danger">Tools require the IGRIS desktop backend.</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-4xl p-8">
      <PageHeader
        title="Tools"
        description="The only actions IGRIS can take. Every call is validated, checked against your permission policy and written to the audit log."
      />

      {notice && (
        <p role="status" className={`mb-4 rounded-lg border px-3 py-2 text-xs ${notice.ok ? "border-success/30 bg-success/10 text-success" : "border-danger/30 bg-danger/10 text-danger"}`}>
          {notice.text}
        </p>
      )}

      <div className="space-y-4">
        <Panel title={`Available tools (${tools.length})`}>
          <ul className="divide-y divide-line">
            {tools.map((t) => (
              <li key={t.name} className="flex items-start gap-4 py-3 first:pt-0 last:pb-0">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm text-fg">{t.title}</span>
                    <code className="font-mono text-[10px] text-faint">{t.name}</code>
                  </div>
                  <p className="mt-0.5 line-clamp-2 text-xs text-muted" title={t.description}>
                    {t.description}
                  </p>
                </div>
                <div className="flex shrink-0 flex-col items-end gap-1">
                  <PermissionBadge level={t.permission} />
                  <span className="text-[10px] text-faint">{t.requiresApproval ? "Asks first" : "Runs automatically"}</span>
                </div>
              </li>
            ))}
          </ul>
        </Panel>

        <SharedFolders onNotice={onFolderNotice} />

        <Panel
          title="Applications IGRIS may open"
          action={
            <button
              type="button"
              onClick={() => void scan()}
              disabled={scanning}
              className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40"
            >
              <ScanSearch className="size-3.5" />
              {scanning ? "Scanning…" : "Find installed apps"}
            </button>
          }
        >
          <p className="mb-3 text-xs text-muted">
            IGRIS can only open applications on this list, by name. It never receives paths, arguments or a command line.
          </p>

          {candidates && (
            <div className="mb-4 rounded-lg border border-line bg-surface p-3">
              <p className="text-label mb-2">Found on this computer</p>
              {candidates.length === 0 ? (
                <p className="text-xs text-faint">No additional well-known apps found. Add one manually below.</p>
              ) : (
                <div className="flex flex-wrap gap-2">
                  {candidates.map((c) => (
                    <button
                      key={c.path}
                      type="button"
                      title={c.path}
                      onClick={() => void add(c.name, c.path)}
                      className="flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs text-fg hover:border-accent hover:text-accent"
                    >
                      <Plus className="size-3" /> {c.name}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}

          {apps.length === 0 ? (
            <p className="py-2 text-sm text-faint">No applications yet.</p>
          ) : (
            <ul className="divide-y divide-line">
              {apps.map((a) => (
                <li key={a.id} className="flex items-center gap-3 py-2.5">
                  <AppWindow className="size-4 shrink-0 text-muted" />
                  <div className="min-w-0 flex-1">
                    <div className="text-sm text-fg">{a.name}</div>
                    <div className="truncate font-mono text-[10px] text-faint" title={a.path} data-selectable>
                      {a.path}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => void launch(a)}
                    disabled={busyId === a.id}
                    className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted hover:bg-surface-hover hover:text-fg disabled:opacity-40"
                  >
                    <Play className="size-3" /> {busyId === a.id ? "Opening…" : "Open"}
                  </button>
                  {confirmRemove === a.id ? (
                    <button type="button" onClick={() => void remove(a)} onMouseLeave={() => setConfirmRemove(null)} className="rounded-md px-2 py-1 text-xs font-medium text-danger hover:bg-danger/10">
                      Remove?
                    </button>
                  ) : (
                    <button type="button" aria-label={`Remove ${a.name}`} onClick={() => setConfirmRemove(a.id)} className="rounded-md p-1.5 text-faint hover:text-danger">
                      <Trash2 className="size-3.5" />
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}

          <form
            className="mt-4 flex flex-wrap items-end gap-2 border-t border-line pt-4"
            onSubmit={async (e) => {
              e.preventDefault();
              if (await add(name, path)) {
                setName("");
                setPath("");
              }
            }}
          >
            <label className="flex w-40 flex-col gap-1 text-[11px] text-muted">
              Name
              <input
                value={name}
                maxLength={60}
                onChange={(e) => setName(e.target.value)}
                placeholder="VS Code"
                className="h-8 rounded-lg border border-line bg-surface px-2.5 text-sm text-fg placeholder:text-faint focus:border-accent focus:outline-none"
              />
            </label>
            <label className="flex min-w-48 flex-1 flex-col gap-1 text-[11px] text-muted">
              Executable path
              <div className="flex gap-1">
                <input
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  spellCheck={false}
                  placeholder={platform === "windows" ? "C:\\Program Files\\App\\app.exe" : "/usr/bin/app"}
                  className="h-8 min-w-0 flex-1 rounded-lg border border-line bg-surface px-2.5 font-mono text-xs text-fg placeholder:text-faint focus:border-accent focus:outline-none"
                />
                <button type="button" onClick={() => void browse()} className="flex h-8 items-center gap-1 rounded-lg border border-line px-2.5 text-xs text-muted hover:bg-surface-hover hover:text-fg">
                  <FolderOpen className="size-3.5" /> Browse…
                </button>
              </div>
            </label>
            <button type="submit" disabled={!name.trim() || !path.trim()} className="h-8 rounded-lg bg-accent px-3 text-xs font-semibold text-bg disabled:opacity-40">
              Add
            </button>
          </form>
        </Panel>
      </div>
    </div>
  );
}
