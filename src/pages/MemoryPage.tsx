import { Brain, Check, Pencil, Search, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Panel } from "@/components/ui/Panel";
import { Segmented } from "@/components/ui/Segmented";
import { Toggle } from "@/components/ui/Toggle";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { MEMORY_KIND_LABEL, MEMORY_MAX_CHARS, type Memory, type MemoryKind } from "@/types/memory";

type Filter = "all" | MemoryKind;

function formatDate(iso: string | null): string {
  if (!iso) return "never";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "—" : d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

export function MemoryPage() {
  const backend = useAppStore((s) => s.backend);
  const { settings, status, saving, update } = useSettingsStore();
  const [items, setItems] = useState<Memory[]>([]);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [reloadKey, setReloadKey] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newKind, setNewKind] = useState<MemoryKind>("long_term");
  const [draft, setDraft] = useState("");

  useEffect(() => {
    const t = setTimeout(() => setDebounced(query), 250);
    return () => clearTimeout(t);
  }, [query]);

  useEffect(() => {
    if (backend !== "ready") return;
    let live = true;
    api
      .listMemories(filter === "all" ? undefined : filter, debounced || undefined)
      .then((list) => {
        if (!live) return;
        setItems(list);
        setLoaded(true);
      })
      .catch((err) => live && setError(BackendError.from(err).message));
    return () => {
      live = false;
    };
  }, [backend, filter, debounced, reloadKey]);

  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
      setReloadKey((k) => k + 1);
      return true;
    } catch (err) {
      setError(BackendError.from(err).message);
      return false;
    }
  };

  if (backend !== "ready") {
    return (
      <div className="mx-auto max-w-3xl px-8 pt-8">
        <PageHeader title="Memory" />
        <p className="text-sm text-danger">Memory requires the IGRIS desktop backend.</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl px-8 pt-8 pb-16">
      <PageHeader
        title="Memory"
        description="What IGRIS remembers between conversations. Everything here is stored only on this computer."
        action={
          <label className="flex shrink-0 items-center gap-2 text-xs whitespace-nowrap text-muted">
            Use memory
            <Toggle
              label="Use memory"
              checked={settings.memoryEnabled}
              disabled={status !== "ready" || saving}
              onChange={(memoryEnabled) => void update({ memoryEnabled })}
            />
          </label>
        }
      />

      {!settings.memoryEnabled && (
        <p className="mb-4 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          Memory is off. IGRIS won't read or save memories in conversations. Your saved memories are kept.
        </p>
      )}
      {error && (
        <p role="alert" className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}

      <div className="space-y-4">
        <Panel title="Add a memory">
          <form
            className="space-y-2"
            onSubmit={async (e) => {
              e.preventDefault();
              if (await run(() => api.addMemory(newKind, draft))) setDraft("");
            }}
          >
            <Segmented<MemoryKind>
              label="Memory type"
              value={newKind}
              onChange={setNewKind}
              options={[
                { value: "long_term", label: MEMORY_KIND_LABEL.long_term },
                { value: "knowledge", label: MEMORY_KIND_LABEL.knowledge },
              ]}
            />
            <div className="flex gap-2">
              <input
                value={draft}
                maxLength={MEMORY_MAX_CHARS}
                onChange={(e) => setDraft(e.target.value)}
                aria-label="New memory"
                placeholder={newKind === "long_term" ? "e.g. My main project is SkillTrack (Java, MySQL)" : "e.g. The staging server is staging.example.com"}
                className="h-9 min-w-0 flex-1 rounded-xl bg-surface-strong px-3 text-sm text-fg placeholder:text-faint transition-shadow duration-200 focus:ring-2 focus:ring-accent/50 focus:outline-none"
              />
              <button type="submit" disabled={!draft.trim()} className="h-9 rounded-lg bg-accent px-4 text-xs font-semibold text-bg disabled:opacity-40">
                Save
              </button>
            </div>
            <p className="text-[11px] text-faint">Passwords, keys, card numbers and ID numbers are never stored.</p>
          </form>
        </Panel>

        <Panel
          title={`Saved memories${loaded ? ` (${items.length})` : ""}`}
          action={
            <Segmented<Filter>
              label="Filter memories"
              value={filter}
              onChange={setFilter}
              options={[
                { value: "all", label: "All" },
                { value: "long_term", label: MEMORY_KIND_LABEL.long_term },
                { value: "knowledge", label: MEMORY_KIND_LABEL.knowledge },
              ]}
            />
          }
        >
          <div className="relative mb-3">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-faint" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              aria-label="Search memories"
              placeholder="Search memories…"
              className="h-9 w-full rounded-xl bg-surface-strong pr-3 pl-8 text-sm text-fg placeholder:text-faint transition-shadow duration-200 focus:ring-2 focus:ring-accent/50 focus:outline-none"
            />
          </div>

          {loaded && items.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-8 text-center">
              <Brain className="size-6 text-faint" />
              <p className="text-sm text-muted">{debounced ? "No memories match that search." : "Nothing saved yet."}</p>
              {!debounced && (
                <p className="text-xs text-faint">Try telling IGRIS: “Remember that my main project is called SkillTrack.”</p>
              )}
            </div>
          ) : (
            <ul className="divide-y divide-line">
              {items.map((m) => (
                <MemoryRow
                  key={m.id}
                  memory={m}
                  onSave={(content) => run(() => api.updateMemory(m.id, content))}
                  onDelete={() => run(() => api.deleteMemory(m.id))}
                />
              ))}
            </ul>
          )}
        </Panel>
      </div>
    </div>
  );
}

function MemoryRow({ memory: m, onSave, onDelete }: { memory: Memory; onSave: (c: string) => Promise<boolean>; onDelete: () => Promise<boolean> }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(m.content);
  const [confirm, setConfirm] = useState(false);

  return (
    <li className="group py-3">
      {editing ? (
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            if (await onSave(draft)) setEditing(false);
          }}
        >
          <input
            value={draft}
            autoFocus
            maxLength={MEMORY_MAX_CHARS}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setEditing(false)}
            aria-label={`Edit memory #${m.id}`}
            className="h-8 min-w-0 flex-1 rounded-lg border border-accent bg-surface px-2.5 text-sm text-fg focus:outline-none"
          />
          <button type="submit" aria-label="Save memory" className="grid size-8 place-items-center rounded-lg text-success hover:bg-surface-hover">
            <Check className="size-4" />
          </button>
          <button type="button" aria-label="Cancel edit" onClick={() => (setEditing(false), setDraft(m.content))} className="grid size-8 place-items-center rounded-lg text-muted hover:bg-surface-hover">
            <X className="size-4" />
          </button>
        </form>
      ) : (
        <div className="flex items-start gap-3">
          <p className="min-w-0 flex-1 text-sm text-fg" data-selectable>
            {m.content}
          </p>
          <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
            <button type="button" aria-label={`Edit memory #${m.id}`} onClick={() => setEditing(true)} className="grid size-7 place-items-center rounded-md text-faint hover:text-fg">
              <Pencil className="size-3.5" />
            </button>
            {confirm ? (
              <button type="button" onClick={() => void onDelete()} onMouseLeave={() => setConfirm(false)} className="rounded-md px-2 py-1 text-xs font-medium text-danger hover:bg-danger/10">
                Delete?
              </button>
            ) : (
              <button type="button" aria-label={`Delete memory #${m.id}`} onClick={() => setConfirm(true)} className="grid size-7 place-items-center rounded-md text-faint hover:text-danger">
                <Trash2 className="size-3.5" />
              </button>
            )}
          </div>
        </div>
      )}
      <div className="mt-1 flex flex-wrap gap-x-3 text-[10px] text-faint">
        <span className="font-mono">#{m.id}</span>
        <span>{MEMORY_KIND_LABEL[m.kind]}</span>
        <span>Saved by {m.source === "user" ? "you" : "IGRIS"} · {formatDate(m.createdAt)}</span>
        <span>
          Used {m.useCount} time{m.useCount === 1 ? "" : "s"}
          {m.lastUsedAt ? ` · last ${formatDate(m.lastUsedAt)}` : ""}
        </span>
      </div>
    </li>
  );
}
