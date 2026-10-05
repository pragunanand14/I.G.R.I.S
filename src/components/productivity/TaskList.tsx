import { Check, ChevronDown, ChevronRight, Flag, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Panel } from "@/components/ui/Panel";
import { useNow } from "@/hooks/useNow";
import { useTimePreview } from "@/hooks/useTimePreview";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { useAlertStore } from "@/stores/alertStore";
import { toTaskInput, type Priority, type Task } from "@/types/productivity";
import { formatWhen } from "@/utils/time";
import { INPUT } from "./styles";
import { TimeHint } from "./TimeHint";


const PRIORITY_STYLE: Record<Priority, string> = { high: "text-danger", normal: "text-faint", low: "text-faint/50" };

function DueBadge({ task, now }: { task: Task; now: number }) {
  if (!task.dueAt) return null;
  const overdue = !task.done && new Date(task.dueAt).getTime() < now;
  return (
    <span className={`shrink-0 rounded-md px-1.5 py-0.5 text-[10px] ${overdue ? "bg-danger/10 text-danger" : "text-muted"}`}>
      {overdue ? "Overdue · " : ""}
      {formatWhen(task.dueAt)}
    </span>
  );
}

function PrioritySelect({ value, onChange }: { value: Priority; onChange: (p: Priority) => void }) {
  return (
    <select aria-label="Priority" value={value} onChange={(e) => onChange(e.target.value as Priority)} className={`${INPUT} w-24 px-1.5`}>
      <option value="low">Low</option>
      <option value="normal">Normal</option>
      <option value="high">High</option>
    </select>
  );
}

/** Resolve a natural-language due text to UTC ISO ("" = no due date); throws a readable error. */
async function resolveDue(text: string): Promise<string> {
  if (!text.trim()) return "";
  return (await api.parseTime(text)).at;
}

function TaskEditor({ task, onDone }: { task: Task; onDone: (err?: string) => void }) {
  const [title, setTitle] = useState(task.title);
  const [notes, setNotes] = useState(task.notes);
  const [priority, setPriority] = useState<Priority>(task.priority);
  const [due, setDue] = useState("");
  const [clearDue, setClearDue] = useState(false);
  const preview = useTimePreview(due);

  const save = async () => {
    try {
      const dueAt = clearDue ? "" : due.trim() ? await resolveDue(due) : (task.dueAt ?? "");
      await api.updateTask(task.id, { ...toTaskInput(task), title, notes, priority, dueAt });
      onDone();
    } catch (err) {
      onDone(BackendError.from(err).message);
    }
  };

  return (
    <div className="space-y-2 rounded-lg border border-line bg-surface-hover/40 p-3">
      <input aria-label="Title" value={title} onChange={(e) => setTitle(e.target.value)} className={`${INPUT} w-full`} />
      <div className="flex flex-wrap items-center gap-2">
        <input
          aria-label="New due date"
          value={due}
          onChange={(e) => setDue(e.target.value)}
          disabled={clearDue}
          placeholder={task.dueAt ? `Due ${formatWhen(task.dueAt)} — type to change` : "Due… e.g. friday 5pm"}
          className={`${INPUT} min-w-0 flex-1`}
        />
        <PrioritySelect value={priority} onChange={setPriority} />
        {task.dueAt && (
          <label className="flex items-center gap-1 text-[11px] text-muted">
            <input type="checkbox" checked={clearDue} onChange={(e) => setClearDue(e.target.checked)} /> No due date
          </label>
        )}
      </div>
      {!clearDue && <TimeHint preview={preview} />}
      <textarea aria-label="Notes" value={notes} onChange={(e) => setNotes(e.target.value)} rows={2} placeholder="Notes" className={`${INPUT} h-auto w-full resize-y py-1.5`} />
      <div className="flex justify-end gap-2">
        <button type="button" onClick={() => onDone()} className="rounded-lg px-3 py-1 text-xs text-muted hover:bg-surface-hover">
          Cancel
        </button>
        <button type="button" disabled={!title.trim() || preview.state === "error"} onClick={() => void save()} className="rounded-lg bg-accent px-3 py-1 text-xs font-semibold text-bg disabled:opacity-40">
          Save
        </button>
      </div>
    </div>
  );
}

export function TaskList({ onError }: { onError: (msg: string | null) => void }) {
  const revision = useAlertStore((s) => s.revision);
  const now = useNow(60_000);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [reload, setReload] = useState(0);
  const [showDone, setShowDone] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);
  const [title, setTitle] = useState("");
  const [due, setDue] = useState("");
  const [priority, setPriority] = useState<Priority>("normal");
  const preview = useTimePreview(due);

  useEffect(() => {
    let live = true;
    api
      .listTasks("all")
      .then((t) => live && setTasks(t))
      .catch((err) => live && onError(BackendError.from(err).message));
    return () => {
      live = false;
    };
  }, [reload, revision, onError]);

  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      onError(null);
    } catch (err) {
      onError(BackendError.from(err).message);
    } finally {
      setReload((k) => k + 1);
    }
  };

  const add = () =>
    run(async () => {
      await api.addTask({ title, notes: "", dueAt: await resolveDue(due), priority });
      setTitle("");
      setDue("");
      setPriority("normal");
    });

  const open = tasks.filter((t) => !t.done);
  const done = tasks.filter((t) => t.done);

  const row = (t: Task) =>
    editing === t.id ? (
      <li key={t.id} className="py-2">
        <TaskEditor
          task={t}
          onDone={(err) => {
            setEditing(null);
            onError(err ?? null);
            setReload((k) => k + 1);
          }}
        />
      </li>
    ) : (
      <li key={t.id} className="group flex items-center gap-3 py-2">
        <button
          type="button"
          role="checkbox"
          aria-checked={t.done}
          aria-label={`${t.done ? "Reopen" : "Complete"} ${t.title}`}
          onClick={() => void run(() => api.setTaskDone(t.id, !t.done))}
          className={`grid size-4.5 shrink-0 place-items-center rounded-md border ${t.done ? "border-accent bg-accent text-bg" : "border-line-strong hover:border-accent"}`}
        >
          {t.done && <Check className="size-3" strokeWidth={3} />}
        </button>
        <div className="min-w-0 flex-1">
          <div className={`truncate text-sm ${t.done ? "text-faint line-through" : "text-fg"}`}>{t.title}</div>
          {t.notes && <div className="truncate text-[11px] text-faint">{t.notes}</div>}
        </div>
        {t.priority !== "normal" && !t.done && <Flag aria-label={`${t.priority} priority`} className={`size-3.5 shrink-0 ${PRIORITY_STYLE[t.priority]}`} />}
        <DueBadge task={t} now={now} />
        <div className="flex shrink-0 gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
          {!t.done && (
            <button type="button" aria-label={`Edit ${t.title}`} onClick={() => setEditing(t.id)} className="grid size-7 place-items-center rounded-md text-faint hover:text-fg">
              <Pencil className="size-3.5" />
            </button>
          )}
          <button type="button" aria-label={`Delete ${t.title}`} onClick={() => void run(() => api.deleteTask(t.id))} className="grid size-7 place-items-center rounded-md text-faint hover:text-danger">
            <Trash2 className="size-3.5" />
          </button>
        </div>
      </li>
    );

  return (
    <div className="space-y-4">
      <Panel title="New task">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim() && preview.state !== "error") void add();
          }}
          className="space-y-2"
        >
          <div className="flex flex-wrap gap-2">
            <input aria-label="Task" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="What needs doing?" className={`${INPUT} min-w-48 flex-[2]`} />
            <input aria-label="Due" value={due} onChange={(e) => setDue(e.target.value)} placeholder="Due (optional) — e.g. friday 5pm" className={`${INPUT} min-w-40 flex-1`} />
            <PrioritySelect value={priority} onChange={setPriority} />
            <button type="submit" disabled={!title.trim() || preview.state === "error"} className="flex h-8 items-center gap-1 rounded-lg bg-accent px-3 text-xs font-semibold text-bg disabled:opacity-40">
              <Plus className="size-3.5" /> Add
            </button>
          </div>
          <TimeHint preview={preview} />
        </form>
      </Panel>

      <Panel title={`Open · ${open.length}`}>
        {open.length === 0 ? <p className="py-1 text-sm text-faint">Nothing on your list. Add a task above or ask IGRIS.</p> : <ul className="divide-y divide-line">{open.map(row)}</ul>}
      </Panel>

      {done.length > 0 && (
        <Panel
          title={
            <button type="button" onClick={() => setShowDone((v) => !v)} className="flex items-center gap-1">
              {showDone ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />} Completed · {done.length}
            </button>
          }
          action={
            showDone && (
              <button type="button" onClick={() => void run(api.clearDoneTasks)} className="text-xs text-muted hover:text-danger">
                Clear completed
              </button>
            )
          }
        >
          {showDone ? <ul className="divide-y divide-line">{done.map(row)}</ul> : <p className="text-xs text-faint">Hidden.</p>}
        </Panel>
      )}
    </div>
  );
}
