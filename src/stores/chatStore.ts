import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { newRequestId, startTurn } from "@/services/chat";
import type { AiStatus } from "@/types/ai";
import type { ChatEvent, Conversation, Message } from "@/types/chat";
import type { ToolActivity } from "@/types/tools";
import { useAssistantStore } from "./assistantStore";

export interface StreamingState {
  requestId: string;
  /** null until the backend creates the conversation. */
  conversationId: string | null;
  text: string;
  phase: "waiting" | "streaming";
  model: string | null;
  /** Tool calls in this turn, in order (upserted by id). */
  activities: ToolActivity[];
}

interface ChatStore {
  conversations: Conversation[];
  activeId: string | null;
  messages: Message[];
  loading: boolean;
  /** User text shown immediately, before the backend confirms it was saved. */
  pendingUser: string | null;
  streaming: StreamingState | null;
  error: string | null;
  errorKind: string | null;
  aiStatus: AiStatus | null;

  loadConversations: () => Promise<void>;
  loadAiStatus: () => Promise<void>;
  setAiStatus: (status: AiStatus) => void;
  openConversation: (id: string | null) => Promise<void>;
  /** Resolves `true` once the message is saved, `false` if it was rejected. */
  send: (content: string, attachmentIds?: string[]) => Promise<boolean>;
  regenerate: () => Promise<void>;
  edit: (messageId: string, content: string) => Promise<boolean>;
  cancel: () => Promise<void>;
  /** Answer a tool approval prompt. */
  answerApproval: (callId: string, approved: boolean) => Promise<void>;
  rename: (id: string, title: string) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  dismissError: () => void;
}

function upsert(list: Conversation[], c: Conversation): Conversation[] {
  return [c, ...list.filter((x) => x.id !== c.id)];
}

export const useChatStore = create<ChatStore>((set, get) => {
  const setActivity = useAssistantStore.getState().setActivity;

  const fail = (err: unknown) => {
    const e = BackendError.from(err);
    set({ error: e.message, errorKind: e.kind });
  };

  /** Shared event handling for send / regenerate / edit. */
  const handleEvent = (requestId: string, ev: ChatEvent, onSaved?: () => void) => {
    const s = get();
    if (s.streaming?.requestId !== requestId) return;
    switch (ev.type) {
      case "userMessage": {
        const isActive = s.activeId === null || s.activeId === ev.conversation.id;
        set({
          conversations: upsert(s.conversations, ev.conversation),
          streaming: { ...s.streaming, conversationId: ev.conversation.id },
          pendingUser: null,
          ...(isActive ? { activeId: ev.conversation.id, messages: [...s.messages, ev.message] } : {}),
        });
        onSaved?.();
        break;
      }
      case "generating":
        set({ streaming: { ...s.streaming, conversationId: ev.conversationId, model: ev.model } });
        setActivity("thinking");
        break;
      case "delta":
        if (useAssistantStore.getState().activity !== "speaking") setActivity("speaking");
        set({ streaming: { ...s.streaming, phase: "streaming", text: s.streaming.text + ev.text } });
        break;
      case "tool": {
        const list = s.streaming.activities;
        const idx = list.findIndex((a) => a.id === ev.activity.id);
        const activities = idx >= 0 ? list.map((a, i) => (i === idx ? ev.activity : a)) : [...list, ev.activity];
        set({ streaming: { ...s.streaming, activities } });
        const st = ev.activity.status;
        setActivity(st === "running" ? "executing" : st === "awaitingApproval" ? "listening" : "thinking");
        break;
      }
      case "finished": {
        const isActive = s.activeId === ev.message.conversationId;
        set({
          streaming: null,
          messages: isActive ? [...s.messages, ev.message] : s.messages,
          conversations: s.conversations.map((c) =>
            c.id === ev.message.conversationId ? { ...c, updatedAt: ev.message.createdAt, messageCount: c.messageCount + 1 } : c,
          ),
        });
        setActivity("idle");
        break;
      }
    }
  };

  /** Run a turn; resolves when the turn ends. */
  const runTurn = async (start: Parameters<typeof startTurn>[1], conversationId: string | null, onSaved?: () => void) => {
    const requestId = newRequestId();
    set({ streaming: { requestId, conversationId, text: "", phase: "waiting", model: null, activities: [] }, error: null, errorKind: null });
    setActivity("thinking");
    try {
      await startTurn(requestId, start, (ev) => handleEvent(requestId, ev, onSaved));
      return true;
    } catch (err) {
      fail(err);
      if (get().streaming?.requestId === requestId) set({ streaming: null, pendingUser: null });
      setActivity("idle");
      return false;
    } finally {
      // The conversation list's ordering/counts may have changed.
      void get().loadConversations();
    }
  };

  return {
    conversations: [],
    activeId: null,
    messages: [],
    loading: false,
    pendingUser: null,
    streaming: null,
    error: null,
    errorKind: null,
    aiStatus: null,

    loadConversations: async () => {
      try {
        set({ conversations: await api.listConversations() });
      } catch (err) {
        fail(err);
      }
    },

    loadAiStatus: async () => {
      try {
        set({ aiStatus: await api.getAiStatus() });
      } catch (err) {
        fail(err);
      }
    },

    setAiStatus: (aiStatus) => set({ aiStatus }),

    openConversation: async (id) => {
      if (id === null) {
        set({ activeId: null, messages: [], error: null, errorKind: null });
        return;
      }
      set({ loading: true, activeId: id, error: null, errorKind: null });
      try {
        const detail = await api.getConversation(id);
        if (get().activeId === id) set({ messages: detail.messages, loading: false });
      } catch (err) {
        fail(err);
        set({ loading: false, activeId: null, messages: [] });
      }
    },

    send: (content, attachmentIds = []) =>
      new Promise<boolean>((resolve) => {
        if (get().streaming) return resolve(false);
        let saved = false;
        set({ pendingUser: content });
        void runTurn({ kind: "send", conversationId: get().activeId, content, attachmentIds }, get().activeId, () => {
          saved = true;
          resolve(true);
        }).then(() => {
          if (!saved) resolve(false);
        });
      }),

    regenerate: async () => {
      const { activeId, streaming, messages } = get();
      if (!activeId || streaming) return;
      // Mirror the backend: drop assistant turns after the last user message.
      const lastUser = messages.map((m) => m.role).lastIndexOf("user");
      set({ messages: messages.slice(0, lastUser + 1) });
      await runTurn({ kind: "regenerate", conversationId: activeId }, activeId);
    },

    edit: async (messageId, content) => {
      const { activeId, streaming, messages } = get();
      if (!activeId || streaming) return false;
      const idx = messages.findIndex((m) => m.id === messageId);
      if (idx < 0) return false;
      const previous = messages;
      set({ messages: [...messages.slice(0, idx), { ...messages[idx]!, content }] });
      const ok = await runTurn({ kind: "edit", messageId, content }, activeId);
      // If the edit was rejected outright, restore the real history.
      if (!ok && get().activeId === activeId) await get().openConversation(activeId).catch(() => set({ messages: previous }));
      return ok;
    },

    cancel: async () => {
      const s = get().streaming;
      if (!s) return;
      try {
        await api.cancelChat(s.requestId);
      } catch (err) {
        fail(err);
      }
    },

    answerApproval: async (callId, approved) => {
      try {
        const delivered = await api.respondToolApproval(callId, approved);
        if (!delivered) set({ error: "That request is no longer waiting for an answer.", errorKind: "validation" });
      } catch (err) {
        fail(err);
      }
    },

    rename: async (id, title) => {
      try {
        const c = await api.renameConversation(id, title);
        set({ conversations: get().conversations.map((x) => (x.id === id ? c : x)) });
        return true;
      } catch (err) {
        fail(err);
        return false;
      }
    },

    remove: async (id) => {
      try {
        await api.deleteConversation(id);
        set({ conversations: get().conversations.filter((c) => c.id !== id) });
        if (get().activeId === id) set({ activeId: null, messages: [] });
        return true;
      } catch (err) {
        fail(err);
        return false;
      }
    },

    dismissError: () => set({ error: null, errorKind: null }),
  };
});
