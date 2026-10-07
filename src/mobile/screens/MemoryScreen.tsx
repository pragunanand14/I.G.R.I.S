import { Plus, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAppStore } from "@/stores/appStore";
import { MEMORY_KIND_LABEL, MEMORY_MAX_CHARS, type Memory } from "@/types/memory";
import { Screen } from "../ui";

/** What IGRIS remembers: search, add, delete. */
export function MemoryScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [items, setItems] = useState<Memory[] | null>(null);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<number | null>(null);

  const load = useCallback(async (q: string) => {
    try {
      setItems(await api.listMemories(undefined, q.trim() || undefined));
      setError(null);
    } catch (err) {
      setError(BackendError.from(err).message);
    }
  }, []);

  useEffect(() => {
    if (!ready) return;
    const t = setTimeout(() => void load(query), 250);
    return () => clearTimeout(t);
  }, [ready, query, load]);

  const add = async (e: FormEvent) => {
    e.preventDefault();
    const content = draft.trim();
    if (!content) return;
    try {
      await api.addMemory("long_term", content);
      setDraft("");
      await load(query);
    } catch (err) {
      setError(BackendError.from(err).message);
    }
  };

  const remove = async (id: number) => {
    try {
      await api.deleteMemory(id);
      setConfirm(null);
      await load(query);
    } catch (err) {
      setError(BackendError.from(err).message);
    }
  };

  return (
    <Screen title="Memories" subtitle="Things IGRIS remembers to help you. Only you can see and change them." back="/more">
      {!ready ? (
        <p className="m-muted">Memories appear here once IGRIS is running.</p>
      ) : (
        <>
          <form onSubmit={(e) => void add(e)} className="m-card flex items-center gap-2 p-3">
            <input
              className="m-input flex-1"
              value={draft}
              maxLength={MEMORY_MAX_CHARS}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Something IGRIS should know about you"
              aria-label="New memory"
            />
            <button type="submit" className="m-button m-button-primary px-4" aria-label="Save memory" disabled={!draft.trim()}>
              <Plus className="size-5" />
            </button>
          </form>

          <div className="relative">
            <Search className="m-faint pointer-events-none absolute top-1/2 left-4 size-5 -translate-y-1/2" />
            <input className="m-input pl-12" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search memories" aria-label="Search memories" />
          </div>

          {error && (
            <p role="alert" className="text-sm" style={{ color: "var(--m-bad)" }}>
              {error}
            </p>
          )}

          <ul className="m-card" aria-label="Memories">
            {items?.length === 0 && <li className="m-row m-muted">{query ? "No memories match." : "IGRIS hasn't remembered anything yet."}</li>}
            {items?.map((m) => (
              <li key={m.id} className="m-row items-start">
                <div className="min-w-0 flex-1">
                  <p className="break-words">{m.content}</p>
                  <p className="m-faint mt-1 text-sm">
                    {MEMORY_KIND_LABEL[m.kind]} · {m.source === "user" ? "added by you" : "saved by IGRIS"}
                  </p>
                </div>
                {confirm === m.id ? (
                  <div className="flex shrink-0 flex-col gap-1">
                    <button type="button" className="m-button min-h-10 px-3 text-sm" style={{ color: "var(--m-bad)" }} onClick={() => void remove(m.id)}>
                      Delete
                    </button>
                    <button type="button" className="m-button min-h-10 px-3 text-sm" onClick={() => setConfirm(null)}>
                      Keep
                    </button>
                  </div>
                ) : (
                  <button type="button" className="m-faint grid size-11 shrink-0 place-items-center" aria-label="Delete memory" onClick={() => setConfirm(m.id)}>
                    <Trash2 className="size-5" />
                  </button>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
    </Screen>
  );
}
