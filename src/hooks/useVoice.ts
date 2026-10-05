import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useNavigate } from "react-router";
import { hasBackend } from "@/services/backend";
import { listenContinuously } from "@/services/voice/browserSpeech";
import { parseWakeWord } from "@/services/voice/wakeWord";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useVoiceStore } from "@/stores/voiceStore";

/** Connects voice to chat, the global hotkey, Esc, spoken replies and the wake word. */
export function useVoice(enabled: boolean) {
  const navigate = useNavigate();
  const wakeWord = useSettingsStore((s) => s.settings.wakeWordEnabled);
  const phase = useVoiceStore((s) => s.phase);

  // Voice input goes through the same path as typed input.
  useEffect(() => {
    useVoiceStore.getState().connect(
      async (text) => {
        const chat = useChatStore.getState();
        if (!location.hash.startsWith("#/chat")) {
          void chat.openConversation(null);
          void navigate("/chat");
        }
        return useChatStore.getState().send(text);
      },
      async () => {
        await useChatStore.getState().cancel();
      },
    );
  }, [navigate]);

  useEffect(() => {
    if (enabled) void useVoiceStore.getState().loadStatus();
  }, [enabled]);

  // Global hotkey from the native side, and Esc to stop.
  useEffect(() => {
    if (!enabled || !hasBackend()) return;
    let unlisten: (() => void) | undefined;
    void listen("voice-toggle", () => void useVoiceStore.getState().toggle()).then((u) => (unlisten = u));
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && useVoiceStore.getState().phase !== "idle") useVoiceStore.getState().cancel();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      unlisten?.();
      window.removeEventListener("keydown", onKey);
    };
  }, [enabled]);

  // Speak streamed replies to spoken requests.
  useEffect(() => {
    let prev = useChatStore.getState().streaming;
    return useChatStore.subscribe((s) => {
      const cur = s.streaming;
      if (cur && prev && cur.requestId === prev.requestId && cur.text.length > prev.text.length) {
        useVoiceStore.getState().replyDelta(cur.text.slice(prev.text.length));
      } else if (cur && (!prev || cur.requestId !== prev.requestId) && cur.text) {
        useVoiceStore.getState().replyDelta(cur.text);
      }
      if (prev && !cur) void useVoiceStore.getState().replyFinished();
      prev = cur;
    });
  }, []);

  // Experimental wake word: only while idle, only when supported.
  useEffect(() => {
    if (!enabled || !wakeWord || phase !== "idle") return;
    return listenContinuously(
      (transcript) => {
        const command = parseWakeWord(transcript);
        if (command === null) return;
        const voice = useVoiceStore.getState();
        if (command) {
          useVoiceStore.setState({ replyPending: useSettingsStore.getState().settings.voiceAutoSpeak, lastTranscript: command });
          const chat = useChatStore.getState();
          if (!location.hash.startsWith("#/chat")) {
            void chat.openConversation(null);
            void navigate("/chat");
          }
          void useChatStore.getState().send(command);
        } else {
          void voice.listen();
        }
      },
      (msg) => useVoiceStore.setState({ error: msg }),
    );
  }, [enabled, wakeWord, phase, navigate]);
}
