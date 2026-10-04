import { vi } from "vitest";
import { api } from "@/services/api";
import * as chatService from "@/services/chat";
import { BackendError } from "@/services/backend";
import type { ChatEvent, Conversation, Message, TurnResult } from "@/types/chat";
import { useAssistantStore } from "./assistantStore";
import { useChatStore } from "./chatStore";

const conv: Conversation = { id: "c1", title: "Hi", createdAt: "2026-10-04T10:00:00Z", updatedAt: "2026-10-04T10:00:00Z", messageCount: 1 };
const msg = (over: Partial<Message>): Message => ({
  id: "m",
  conversationId: "c1",
  seq: 1,
  role: "user",
  content: "Hi",
  status: "complete",
  error: null,
  provider: null,
  model: null,
  inputTokens: null,
  outputTokens: null,
  createdAt: "2026-10-04T10:00:00Z",
  toolActivity: null,
  ...over,
});

function reset() {
  useChatStore.setState({ conversations: [], activeId: null, messages: [], pendingUser: null, streaming: null, error: null, errorKind: null, aiStatus: null });
  useAssistantStore.setState({ activity: "idle" });
}

/** Mock startTurn: emits `events` (pausing between them so assertions can observe state), then resolves/rejects. */
function mockTurn(events: ChatEvent[], outcome: { reject?: unknown } = {}) {
  const seen: string[] = [];
  vi.spyOn(chatService, "startTurn").mockImplementation(async (_id, _start, onEvent) => {
    for (const ev of events) {
      onEvent(ev);
      seen.push(`${ev.type}:${useAssistantStore.getState().activity}`);
      await Promise.resolve();
    }
    if (outcome.reject) throw outcome.reject;
    return { conversationId: "c1", assistantMessage: msg({}) } as TurnResult;
  });
  return seen;
}

describe("chat store", () => {
  beforeEach(() => {
    reset();
    vi.restoreAllMocks();
    vi.spyOn(api, "listConversations").mockResolvedValue([conv]);
  });

  it("streams a full turn and drives the core state", async () => {
    const assistant = msg({ id: "a1", seq: 2, role: "assistant", content: "Hello, Ada.", provider: "anthropic", model: "claude-opus-5-5" });
    const seen = mockTurn([
      { type: "userMessage", conversation: conv, message: msg({ id: "u1" }) },
      { type: "generating", conversationId: "c1", model: "claude-opus-5-5" },
      { type: "delta", text: "Hello" },
      { type: "delta", text: ", Ada." },
      { type: "finished", message: assistant },
    ]);
    const accepted = await useChatStore.getState().send("Hi");
    expect(accepted).toBe(true);
    await vi.waitFor(() => expect(useChatStore.getState().streaming).toBeNull());
    const s = useChatStore.getState();
    expect(s.activeId).toBe("c1");
    expect(s.messages.map((m) => m.content)).toEqual(["Hi", "Hello, Ada."]);
    expect(s.pendingUser).toBeNull();
    expect(seen).toEqual(["userMessage:thinking", "generating:thinking", "delta:speaking", "delta:speaking", "finished:idle"]);
  });

  it("reports rejection before saving so the composer can restore the draft", async () => {
    mockTurn([], { reject: new BackendError("ai_unavailable" as never, "AI_API_KEY is not set.") });
    const accepted = await useChatStore.getState().send("Hi");
    expect(accepted).toBe(false);
    const s = useChatStore.getState();
    expect(s.error).toBe("AI_API_KEY is not set.");
    expect(s.streaming).toBeNull();
    expect(s.pendingUser).toBeNull();
    expect(useAssistantStore.getState().activity).toBe("idle");
  });

  it("ignores events from a different request", async () => {
    useChatStore.setState({ streaming: { requestId: "other", conversationId: "c1", text: "", phase: "waiting", model: null, activities: [] } });
    expect(await useChatStore.getState().send("Hi")).toBe(false);
  });

  it("tracks tool activity and drives the executing / approval states", async () => {
    const act = { id: "t1", tool: "calculator", title: "Calculator", permission: "safe" as const, description: "Calculate 6*7", result: null, durationMs: null };
    const seen = mockTurn([
      { type: "userMessage", conversation: conv, message: msg({ id: "u1" }) },
      { type: "tool", activity: { ...act, status: "awaitingApproval" } },
      { type: "tool", activity: { ...act, status: "running" } },
      { type: "tool", activity: { ...act, status: "completed", result: "= 42", durationMs: 3 } },
      { type: "delta", text: "42" },
      {
        type: "finished",
        message: msg({ id: "a1", seq: 2, role: "assistant", content: "42", toolActivity: [{ ...act, status: "completed", result: "= 42", durationMs: 3 }] }),
      },
    ]);
    let mid: unknown = null;
    const unsub = useChatStore.subscribe((st) => {
      if (st.streaming?.activities.length) mid = st.streaming.activities;
    });
    await useChatStore.getState().send("6*7?");
    await vi.waitFor(() => expect(useChatStore.getState().streaming).toBeNull());
    unsub();
    expect(mid).toEqual([{ ...act, status: "completed", result: "= 42", durationMs: 3 }]);
    expect(seen).toEqual(["userMessage:thinking", "tool:listening", "tool:executing", "tool:thinking", "delta:speaking", "finished:idle"]);
    expect(useChatStore.getState().messages.at(-1)?.toolActivity?.[0]?.result).toBe("= 42");
  });

  it("forwards approval answers to the backend", async () => {
    const spy = vi.spyOn(api, "respondToolApproval").mockResolvedValue(true);
    await useChatStore.getState().answerApproval("t1", false);
    expect(spy).toHaveBeenCalledWith("t1", false);
    expect(useChatStore.getState().error).toBeNull();
    spy.mockResolvedValue(false);
    await useChatStore.getState().answerApproval("t1", true);
    expect(useChatStore.getState().error).toMatch(/no longer waiting/);
  });

  it("regenerate trims trailing assistant turns locally", async () => {
    useChatStore.setState({
      activeId: "c1",
      messages: [msg({ id: "u1" }), msg({ id: "a1", seq: 2, role: "assistant", content: "old" })],
    });
    mockTurn([{ type: "finished", message: msg({ id: "a2", seq: 2, role: "assistant", content: "new" }) }]);
    await useChatStore.getState().regenerate();
    expect(useChatStore.getState().messages.map((m) => m.content)).toEqual(["Hi", "new"]);
  });
});
