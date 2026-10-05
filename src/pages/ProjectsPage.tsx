import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderGit2, FolderOpen, GitBranch, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { Toggle } from "@/components/ui/Toggle";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { toInput, type Project, type ProjectInput } from "@/types/workspace";

const EMPTY: ProjectInput = { name: "", path: "", repository: "", language: "", framework: "", description: "", notes: "" };


export function ProjectsPage() {
  const backend = useAppStore((s) => s.backend);
  const [projects, setProjects] = useState<Project[]>([]);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: number | null; input: ProjectInput; branch?: string } | null>(null);
  const [share, setShare] = useState(true);
  const [confirm, setConfirm] = useState<number | null>(null);

  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    api
      .listProjects()
      .then((p) => live && setProjects(p))
      .catch((err) => live && setError(BackendError.from(err).message));
    return () => {
      live = false;
    };
  }, [backend, reload]);

  const detect = async (path: string) => {
    if (!path.trim()) return;
    try {
      const d = await api.detectProject(path.trim());
      setEditing((e) => ({
        id: e?.id ?? null,
        branch: d.branch,
        input: {
          ...(e?.input ?? EMPTY),
          path: path.trim(),
          name: e?.input.name || d.name,
          language: e?.input.language || d.language,
          framework: e?.input.framework || d.framework,
          repository: e?.input.repository || d.repository,
        },
      }));
      setError(null);
    } catch (err) {
      setError(BackendError.from(err).message);
    }
  };

  const chooseFolder = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false, title: "Choose the project folder" });
      if (typeof picked === "string") await detect(picked);
    } catch (err) {
      setError(BackendError.from(err).message);
    }
  };

  const save = async () => {
    if (!editing) return;
    setError(null);
    try {
      if (editing.id === null) {
        await api.addProject(editing.input);
        if (share) {
          // Read-only sharing so file tools can search and read the project; ignore "already shared".
          await api.addFolder(editing.input.path, false).catch(() => undefined);
        }
      } else {
        await api.updateProject(editing.id, editing.input);
      }
      setEditing(null);
      setReload((k) => k + 1);
    } catch (err) {
      setError(BackendError.from(err).message);
    }
  };

  const remove = async (id: number) => {
    try {
      await api.removeProject(id);
      setReload((k) => k + 1);
    } catch (err) {
      setError(BackendError.from(err).message);
    } finally {
      setConfirm(null);
    }
  };

  if (backend !== "ready") {
    return (
      <div className="p-8">
        <PageHeader title="Projects" />
        <p className="text-sm text-danger">Projects require the IGRIS desktop backend.</p>
      </div>
    );
  }

  const field = (key: keyof ProjectInput, label: string, placeholder = "", multiline = false) => (
    <label className="flex flex-col gap-1 text-[11px] text-muted">
      {label}
      {multiline ? (
        <textarea
          value={editing?.input[key] ?? ""}
          onChange={(e) => setEditing((s) => s && { ...s, input: { ...s.input, [key]: e.target.value } })}
          rows={3}
          placeholder={placeholder}
          className="resize-y rounded-lg border border-line bg-surface px-2.5 py-1.5 text-sm text-fg placeholder:text-faint focus:border-accent focus:outline-none"
        />
      ) : (
        <input
          value={editing?.input[key] ?? ""}
          onChange={(e) => setEditing((s) => s && { ...s, input: { ...s.input, [key]: e.target.value } })}
          placeholder={placeholder}
          aria-label={label}
          className="h-8 rounded-lg border border-line bg-surface px-2.5 text-sm text-fg placeholder:text-faint focus:border-accent focus:outline-none"
        />
      )}
    </label>
  );

  return (
    <div className="mx-auto max-w-4xl p-8">
      <PageHeader
        title="Projects"
        description="Register your projects so IGRIS can load their context when you ask about them."
        action={
          !editing && (
            <button type="button" onClick={() => setEditing({ id: null, input: EMPTY })} className="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-bg">
              <Plus className="size-3.5" /> Add project
            </button>
          )
        }
      />
      {error && (
        <p role="alert" className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}

      {editing && (
        <Panel title={editing.id === null ? "New project" : "Edit project"} className="mb-4">
          <div className="space-y-3">
            <div className="flex items-end gap-2">
              <label className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-muted">
                Folder
                <input
                  value={editing.input.path}
                  onChange={(e) => setEditing((s) => s && { ...s, input: { ...s.input, path: e.target.value } })}
                  onBlur={(e) => void detect(e.target.value)}
                  placeholder="C:\Users\you\Projects\SkillTrack"
                  aria-label="Folder"
                  className="h-8 rounded-lg border border-line bg-surface px-2.5 font-mono text-xs text-fg placeholder:text-faint focus:border-accent focus:outline-none"
                />
              </label>
              <button type="button" onClick={() => void chooseFolder()} className="flex h-8 items-center gap-1 rounded-lg border border-line px-2.5 text-xs text-muted hover:bg-surface-hover hover:text-fg">
                <FolderOpen className="size-3.5" /> Choose…
              </button>
            </div>
            {editing.branch && (
              <p className="flex items-center gap-1 text-[11px] text-faint">
                <GitBranch className="size-3" /> Git branch {editing.branch} — details detected from the folder; edit as needed.
              </p>
            )}
            <div className="grid grid-cols-2 gap-3">
              {field("name", "Name", "SkillTrack")}
              {field("language", "Language", "Java")}
              {field("framework", "Framework / stack", "Spring Boot, MySQL")}
              {field("repository", "Repository", "https://github.com/…")}
            </div>
            {field("description", "Description", "What it is", true)}
            {field("notes", "Notes for IGRIS", "Deadlines, conventions, things to remember", true)}
            {editing.id === null && (
              <label className="flex items-center gap-2 text-xs text-muted">
                <Toggle label="Share this folder read-only" checked={share} onChange={setShare} />
                Also share the folder read-only, so IGRIS can search and read its files
              </label>
            )}
            <div className="flex justify-end gap-2">
              <button type="button" onClick={() => setEditing(null)} className="rounded-lg px-3 py-1.5 text-xs text-muted hover:bg-surface-hover">
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void save()}
                disabled={!editing.input.name.trim() || !editing.input.path.trim()}
                className="rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-bg disabled:opacity-40"
              >
                Save project
              </button>
            </div>
          </div>
        </Panel>
      )}

      {projects.length === 0 && !editing ? (
        <div className="flex flex-col items-center gap-2 rounded-xl border border-line bg-surface py-12 text-center">
          <FolderGit2 className="size-6 text-faint" />
          <p className="text-sm text-muted">No projects yet.</p>
          <p className="text-xs text-faint">Add one, then ask IGRIS things like “What's the stack of SkillTrack?”</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {projects.map((p) => (
            <div key={p.id} className="group rounded-xl border border-line bg-surface p-4">
              <div className="flex items-start gap-2">
                <FolderGit2 className="mt-0.5 size-4 shrink-0 text-accent" />
                <div className="min-w-0 flex-1">
                  <div className="text-sm text-fg">{p.name}</div>
                  <div className="truncate font-mono text-[10px] text-faint" title={p.path} data-selectable>
                    {p.path}
                  </div>
                </div>
                <div className="flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                  <button type="button" aria-label={`Edit ${p.name}`} onClick={() => setEditing({ id: p.id, input: toInput(p) })} className="grid size-7 place-items-center rounded-md text-faint hover:text-fg">
                    <Pencil className="size-3.5" />
                  </button>
                  {confirm === p.id ? (
                    <button type="button" onClick={() => void remove(p.id)} onMouseLeave={() => setConfirm(null)} className="rounded-md px-2 text-xs font-medium text-danger hover:bg-danger/10">
                      Remove?
                    </button>
                  ) : (
                    <button type="button" aria-label={`Remove ${p.name}`} onClick={() => setConfirm(p.id)} className="grid size-7 place-items-center rounded-md text-faint hover:text-danger">
                      <Trash2 className="size-3.5" />
                    </button>
                  )}
                </div>
              </div>
              {(p.language || p.framework) && (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {[p.language, ...p.framework.split(",")]
                    .map((t) => t.trim())
                    .filter(Boolean)
                    .map((t) => (
                      <span key={t} className="rounded-md border border-line px-1.5 py-0.5 text-[10px] text-muted">
                        {t}
                      </span>
                    ))}
                </div>
              )}
              {p.description && <p className="mt-2 line-clamp-2 text-xs text-muted">{p.description}</p>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
