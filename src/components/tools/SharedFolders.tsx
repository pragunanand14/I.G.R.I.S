import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Folder, FolderPlus, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Panel } from "@/components/ui/Panel";
import { Toggle } from "@/components/ui/Toggle";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { AllowedFolder, FolderSuggestion } from "@/types/workspace";

/** Folders IGRIS's file tools may access. */
export function SharedFolders({ onNotice }: { onNotice: (n: { ok: boolean; text: string }) => void }) {
  const [folders, setFolders] = useState<AllowedFolder[]>([]);
  const [suggestions, setSuggestions] = useState<FolderSuggestion[]>([]);
  const [reload, setReload] = useState(0);
  const [confirm, setConfirm] = useState<number | null>(null);
  const [newWritable, setNewWritable] = useState(false);

  useEffect(() => {
    let live = true;
    Promise.all([api.listFolders(), api.suggestFolders()])
      .then(([f, s]) => {
        if (!live) return;
        setFolders(f);
        setSuggestions(s);
      })
      .catch((err) => live && onNotice({ ok: false, text: BackendError.from(err).message }));
    return () => {
      live = false;
    };
  }, [reload, onNotice]);

  const run = async (fn: () => Promise<unknown>, ok?: string) => {
    try {
      await fn();
      if (ok) onNotice({ ok: true, text: ok });
      setReload((k) => k + 1);
    } catch (err) {
      onNotice({ ok: false, text: BackendError.from(err).message });
    }
  };

  const pick = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false, title: "Share a folder with IGRIS" });
      if (typeof picked === "string") await run(() => api.addFolder(picked, newWritable), "Folder shared.");
    } catch (err) {
      onNotice({ ok: false, text: BackendError.from(err).message });
    }
  };

  return (
    <Panel
      title="Shared folders"
      action={
        <button type="button" onClick={() => void pick()} className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted hover:bg-surface-hover hover:text-fg">
          <FolderPlus className="size-3.5" /> Share a folder…
        </button>
      }
    >
      <p className="mb-3 text-xs text-muted">
        File tools only work inside these folders. Read-only folders can be listed, searched and read; allow changes to let IGRIS create files
        (overwriting, moving and deleting always ask first). Credential files like <code className="font-mono">.env</code> and SSH keys are never read.
      </p>
      {folders.length === 0 ? (
        <p className="py-1 text-sm text-faint">No folders shared — IGRIS can't access any files.</p>
      ) : (
        <ul className="divide-y divide-line">
          {folders.map((f) => (
            <li key={f.id} className="flex items-center gap-3 py-2.5">
              <Folder className="size-4 shrink-0 text-muted" />
              <span className="min-w-0 flex-1 truncate font-mono text-xs text-fg" title={f.path} data-selectable>
                {f.path}
              </span>
              <label className="flex shrink-0 items-center gap-2 text-[11px] text-muted">
                Allow changes
                <Toggle label={`Allow changes in ${f.path}`} checked={f.writable} onChange={(w) => void run(() => api.setFolderWritable(f.id, w))} />
              </label>
              {confirm === f.id ? (
                <button
                  type="button"
                  onClick={() => void run(() => api.removeFolder(f.id), "Folder no longer shared.")}
                  onMouseLeave={() => setConfirm(null)}
                  className="rounded-md px-2 py-1 text-xs font-medium text-danger hover:bg-danger/10"
                >
                  Stop sharing?
                </button>
              ) : (
                <button type="button" aria-label={`Stop sharing ${f.path}`} onClick={() => setConfirm(f.id)} className="rounded-md p-1.5 text-faint hover:text-danger">
                  <Trash2 className="size-3.5" />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-line pt-3">
        {suggestions.map((s) => (
          <button
            key={s.path}
            type="button"
            title={s.path}
            onClick={() => void run(() => api.addFolder(s.path, newWritable), `${s.label} shared.`)}
            className="flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs text-fg hover:border-accent hover:text-accent"
          >
            <Plus className="size-3" /> {s.label}
          </button>
        ))}
        <label className="ml-auto flex items-center gap-2 text-[11px] text-muted">
          New folders allow changes
          <Toggle label="New folders allow changes" checked={newWritable} onChange={setNewWritable} />
        </label>
      </div>
    </Panel>
  );
}
