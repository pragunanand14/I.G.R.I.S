import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError, hasBackend } from "@/services/backend";
import type { TaskControl, TaskInfo, TaskUpdate } from "@/types/task";

interface TaskStore {
  /** Known tasks by id (from `task-update` events and per-conversation loads). */
  tasks: Record<string, TaskInfo>;
  /** Finished tasks the user closed in this session. */
  hidden: string[];
  error: string | null;
  connect: () => () => void;
  apply: (task: TaskInfo) => void;
  load: (conversationId: string) => Promise<void>;
  control: (taskId: string, action: TaskControl) => Promise<void>;
  hide: (taskId: string) => void;
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: {},
  hidden: [],
  error: null,

  apply: (task) => set({ tasks: { ...get().tasks, [task.id]: task }, error: null }),

  connect: () => {
    if (!hasBackend()) return () => undefined;
    let unlisten: (() => void) | undefined;
    let closed = false;
    void listen<TaskUpdate>("task-update", (e) => get().apply(e.payload.task)).then((u) => {
      if (closed) u();
      else unlisten = u;
    });
    return () => {
      closed = true;
      unlisten?.();
    };
  },

  load: async (conversationId) => {
    try {
      const list = await api.listConversationTasks(conversationId, 5);
      const tasks = { ...get().tasks };
      for (const t of list) tasks[t.id] = t;
      set({ tasks });
    } catch (err) {
      set({ error: BackendError.from(err).message });
    }
  },

  control: async (taskId, action) => {
    try {
      get().apply(await api.taskControl(taskId, action));
    } catch (err) {
      set({ error: BackendError.from(err).message });
    }
  },

  hide: (taskId) => set({ hidden: [...get().hidden, taskId] }),
}));

/** The task to show for a conversation: its newest one, unless the user closed it. */
export function latestTask(tasks: Record<string, TaskInfo>, conversationId: string | null, hidden: string[]): TaskInfo | null {
  if (!conversationId) return null;
  const mine = Object.values(tasks)
    .filter((t) => t.conversationId === conversationId)
    .sort((a, b) => (a.createdAt < b.createdAt ? 1 : a.createdAt > b.createdAt ? -1 : 0));
  const t = mine[0];
  return t && !hidden.includes(t.id) ? t : null;
}
