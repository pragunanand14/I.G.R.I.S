import { AlertTriangle, History, Settings as SettingsIcon, SquarePen, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { Composer } from "@/components/chat/Composer";
import { ConversationList } from "@/components/chat/ConversationList";
import { MessageList } from "@/components/chat/MessageList";
import { TaskCard } from "@/components/chat/TaskCard";
import { AiCore } from "@/components/core/AiCore";
import { useCoreState } from "@/hooks/useCoreState";
import { isMobilePlatform } from "@/services/platform";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";
import { useTaskStore } from "@/stores/taskStore";

// What IGRIS can do on this device (the phone app can't see the screen or use files).
const DESKTOP_ABILITIES =
  "I can search the web, look at images, PDFs and your screen, work with files in folders you share, manage tasks and reminders, open apps you allow and remember what matters.";
const PHONE_ABILITIES =
  "I can search the web, look at images and PDFs, manage tasks and reminders, open your apps and links, check your battery and remember what matters.";

export function ChatPage() {
  const backend = useAppStore((s) => s.backend);
  const coreState = useCoreState();
  const s = useChatStore();
  const { loadConversations, loadAiStatus } = s;

  useEffect(() => {
    if (backend !== "ready") return;
    void loadConversations();
    void loadAiStatus();
  }, [backend, loadConversations, loadAiStatus]);

  const loadTasks = useTaskStore((t) => t.load);
  useEffect(() => {
    if (backend === "ready" && s.activeId) void loadTasks(s.activeId);
  }, [backend, s.activeId, loadTasks]);

  const [historyOpen, setHistoryOpen] = useState(false);
  useEffect(() => {
    if (!historyOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setHistoryOpen(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [historyOpen]);
  const active = s.conversations.find((c) => c.id === s.activeId) ?? null;
  const streamingHere = s.streaming !== null && (s.streaming.conversationId === s.activeId || s.streaming.conversationId === null);
  const streamingElsewhere = s.streaming !== null && !streamingHere;
  const aiReady = s.aiStatus?.ready ?? false;
  const empty = s.messages.length === 0 && !s.pendingUser && !streamingHere;

  const disabledReason =
    backend !== "ready"
      ? "The IGRIS backend is unavailable."
      : !aiReady
        ? "Configure an AI provider to start chatting."
        : streamingElsewhere
          ? "IGRIS is responding in another conversation…"
          : undefined;

  return (
    <div className="relative flex h-full min-h-0 overflow-hidden">
      {historyOpen && (
        <>
          <div className="anim-fade absolute inset-0 z-20 bg-black/30" onClick={() => setHistoryOpen(false)} aria-hidden="true" />
          <div className="drawer-in absolute inset-y-0 left-0 z-30 w-72 shadow-[var(--shadow)]">
            <ConversationList
              conversations={s.conversations}
              activeId={s.activeId}
              busyId={s.streaming?.conversationId ?? null}
              onSelect={(id) => {
                setHistoryOpen(false);
                void s.openConversation(id);
              }}
              onRename={s.rename}
              onDelete={s.remove}
              onClose={() => setHistoryOpen(false)}
            />
          </div>
        </>
      )}

      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between gap-4 px-4 md:px-6">
          <div className="flex min-w-0 items-center gap-2.5">
            <button
              type="button"
              onClick={() => setHistoryOpen(true)}
              aria-label="Conversations"
              title="Conversations"
              className="grid size-8 shrink-0 place-items-center rounded-full text-muted hover:bg-surface-hover hover:text-fg"
            >
              <History className="size-[18px]" />
            </button>
            {!empty && (
              <span className="size-6 shrink-0 [view-transition-name:igris-orb]" aria-hidden="true">
                <AiCore state={coreState} size="100%" />
              </span>
            )}
            <button
              type="button"
              onClick={() => void s.openConversation(null)}
              aria-label="New conversation"
              title="New conversation"
              className="grid size-8 shrink-0 place-items-center rounded-full text-muted hover:bg-surface-hover hover:text-fg"
            >
              <SquarePen className="size-[18px]" />
            </button>
            <h1 key={active?.id ?? "new"} className="anim-fade truncate text-sm font-medium text-fg">
              {active?.title ?? "New conversation"}
            </h1>
          </div>
          {s.aiStatus?.effectiveModel && (
            <span className="shrink-0 rounded-full bg-surface-strong px-2.5 py-1 font-mono text-[10px] text-muted" title="Model used for new messages">
              {s.aiStatus.provider} · {s.aiStatus.effectiveModel}
            </span>
          )}
        </header>

        {s.aiStatus && !s.aiStatus.ready && (
          <div className="mx-6 mt-4 flex items-start gap-3 rounded-lg border border-warning/30 bg-warning/10 px-4 py-3 text-sm text-warning">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            <div className="min-w-0 flex-1">
              <p className="font-medium">AI provider not configured</p>
              <p className="mt-0.5 text-xs opacity-90">{s.aiStatus.problem}</p>
            </div>
            <Link to="/settings" className="flex shrink-0 items-center gap-1 text-xs font-medium hover:underline">
              <SettingsIcon className="size-3.5" /> Settings
            </Link>
          </div>
        )}

        {s.error && (
          <div role="alert" className="mx-6 mt-4 flex items-start gap-3 rounded-lg border border-danger/30 bg-danger/10 px-4 py-2.5 text-sm text-danger">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            <p className="min-w-0 flex-1">{s.error}</p>
            <button type="button" onClick={s.dismissError} aria-label="Dismiss error" className="shrink-0 opacity-70 hover:opacity-100">
              <X className="size-4" />
            </button>
          </div>
        )}

        {empty ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-5 px-6 text-center">
            {s.loading ? (
              <p className="text-sm text-muted">Loading conversation…</p>
            ) : (
              <>
                <span className="size-[140px] [view-transition-name:igris-orb]">
                  <AiCore state={coreState} size="100%" />
                </span>
                <div className="anim-rise">
                  <p className="text-xl font-semibold tracking-tight text-fg">How can I help?</p>
                  <p className="mx-auto mt-2 max-w-lg text-sm text-muted">{isMobilePlatform() ? PHONE_ABILITIES : DESKTOP_ABILITIES}</p>
                </div>
              </>
            )}
          </div>
        ) : (
          <MessageList
            messages={s.messages}
            pendingUser={streamingHere ? s.pendingUser : null}
            streaming={streamingHere ? s.streaming : null}
            onRegenerate={() => void s.regenerate()}
            onEdit={s.edit}
            onAnswerApproval={(id, ok, trust) => void s.answerApproval(id, ok, trust)}
          />
        )}

        <div className="mx-auto w-full max-w-3xl px-3 pb-3 md:px-6 md:pb-5">
          <TaskCard conversationId={s.activeId} busy={s.streaming !== null} onResume={(id, cid) => void s.resumeTask(id, cid)} />
          <Composer
            key={s.activeId ?? "new"}
            onSend={s.send}
            onStop={() => void s.cancel()}
            streaming={streamingHere}
            disabled={disabledReason !== undefined}
            disabledReason={disabledReason}
            autoFocus
            voice={backend === "ready"}
            attachments={backend === "ready"}
          />
          <p className="mt-1.5 text-center text-[10px] text-faint">Enter to send · Shift+Enter for a new line · Paste or drop images and PDFs · AI can make mistakes.</p>
        </div>
      </section>
    </div>
  );
}
