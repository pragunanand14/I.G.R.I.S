import { AlertTriangle, Settings as SettingsIcon, X } from "lucide-react";
import { useEffect } from "react";
import { Link } from "react-router";
import { Composer } from "@/components/chat/Composer";
import { ConversationList } from "@/components/chat/ConversationList";
import { MessageList } from "@/components/chat/MessageList";
import { AiCore } from "@/components/core/AiCore";
import { useCoreState } from "@/hooks/useCoreState";
import { useAppStore } from "@/stores/appStore";
import { useChatStore } from "@/stores/chatStore";

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

  const active = s.conversations.find((c) => c.id === s.activeId) ?? null;
  const streamingHere = s.streaming !== null && (s.streaming.conversationId === s.activeId || s.streaming.conversationId === null);
  const streamingElsewhere = s.streaming !== null && !streamingHere;
  const aiReady = s.aiStatus?.ready ?? false;

  const disabledReason =
    backend !== "ready"
      ? "The IGRIS backend is unavailable."
      : !aiReady
        ? "Configure an AI provider to start chatting."
        : streamingElsewhere
          ? "IGRIS is responding in another conversation…"
          : undefined;

  return (
    <div className="flex h-full min-h-0">
      <ConversationList
        conversations={s.conversations}
        activeId={s.activeId}
        busyId={s.streaming?.conversationId ?? null}
        onSelect={(id) => void s.openConversation(id)}
        onRename={s.rename}
        onDelete={s.remove}
      />

      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between gap-4 border-b border-line px-6">
          <h1 className="truncate text-sm text-fg">{active?.title ?? "New conversation"}</h1>
          {s.aiStatus?.effectiveModel && (
            <span className="shrink-0 rounded-md border border-line px-2 py-0.5 font-mono text-[10px] text-muted" title="Model used for new messages">
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

        {s.messages.length === 0 && !s.pendingUser && !streamingHere ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-4 px-6 text-center">
            {s.loading ? (
              <p className="text-sm text-muted">Loading conversation…</p>
            ) : (
              <>
                <AiCore state={coreState} size={140} />
                <div>
                  <p className="text-base font-light text-fg">How can I help?</p>
                  <p className="mt-1 text-xs text-faint">I can calculate, check your system, open apps you allow and remember what matters. Web access arrives in a later phase.</p>
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
            onAnswerApproval={(id, ok) => void s.answerApproval(id, ok)}
          />
        )}

        <div className="mx-auto w-full max-w-3xl px-6 pb-5">
          <Composer
            key={s.activeId ?? "new"}
            onSend={s.send}
            onStop={() => void s.cancel()}
            streaming={streamingHere}
            disabled={disabledReason !== undefined}
            disabledReason={disabledReason}
            autoFocus
            voice={backend === "ready"}
          />
          <p className="mt-1.5 text-center text-[10px] text-faint">Enter to send · Shift+Enter for a new line · AI can make mistakes.</p>
        </div>
      </section>
    </div>
  );
}
