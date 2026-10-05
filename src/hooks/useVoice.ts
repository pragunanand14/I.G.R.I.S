import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useNavigate } from "react-router";
import { BackendError, hasBackend } from "@/services/backend";
import { listenContinuously } from "@/services/voice/browserSpeech";
import { WakeListener } from "@/services/voice/wakeListener";
import { parseWakeCommand, parseWakeWord, parseYesNo } from "@/services/voice/wakeWord";
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

  const sttMode = useVoiceStore((s) => (s.status ? (s.status.stt.problem ? "unavailable" : s.status.stt.mode) : null));

  // Wake word with the webview's own recogniser (systems where it works).
  useEffect(() => {
    if (!enabled || !wakeWord || sttMode !== "browser" || phase !== "idle") return;
    return listenContinuously(
      (transcript) => {
        const command = parseWakeWord(transcript);
        if (command === null) return;
        if (command) void useVoiceStore.getState().submitText(command);
        else void useVoiceStore.getState().listen();
      },
      (msg) => useVoiceStore.setState({ error: msg }),
    );
  }, [enabled, wakeWord, sttMode, phase]);

  // Always-on wake word via local speech detection + the configured speech service.
  useEffect(() => {
    if (!enabled || !wakeWord || !hasBackend() || !sttMode || sttMode === "browser" || sttMode === "unavailable") return;
    let busy = false;
    let pausedUntil = 0;
    let stopped = false;
    const listener = new WakeListener((wav) => void onPhrase(wav));
    const sync = () => {
      const v = useVoiceStore.getState();
      listener.setPaused(busy || v.phase !== "idle" || v.replyPending || Date.now() < pausedUntil);
    };
    const onPhrase = async (wav: Blob) => {
      const v = useVoiceStore.getState();
      if (busy || v.phase !== "idle" || v.replyPending || useChatStore.getState().streaming || Date.now() < pausedUntil) return;
      busy = true;
      sync();
      try {
        const command = parseWakeCommand(await v.transcribe(wav, "audio/wav"));
        if (command === null || stopped) return; // not addressed to IGRIS
        if (command) await useVoiceStore.getState().submitText(command);
        else {
          await useVoiceStore.getState().say("Yes?");
          await useVoiceStore.getState().listen();
        }
      } catch (err) {
        // e.g. the speech service's rate limit: back off instead of retrying every phrase.
        pausedUntil = Date.now() + 30_000;
        useVoiceStore.setState({ error: `Wake word paused for 30 seconds: ${BackendError.from(err).message}` });
      } finally {
        busy = false;
        sync();
      }
    };
    const unsubscribe = useVoiceStore.subscribe(sync);
    listener
      .start()
      .then(() => !stopped && useVoiceStore.setState({ wakeActive: true }))
      .catch((err) => useVoiceStore.setState({ wakeActive: false, error: err instanceof Error ? err.message : "Couldn't start listening." }));
    return () => {
      stopped = true;
      unsubscribe();
      listener.stop();
      useVoiceStore.setState({ wakeActive: false });
    };
  }, [enabled, wakeWord, sttMode]);

  // Hands-free approvals: when a voice-driven request needs permission, ask out loud.
  useEffect(() => {
    const asked = new Set<string>();
    return useChatStore.subscribe((s) => {
      const pending = s.streaming?.activities.find((a) => a.status === "awaitingApproval" && !asked.has(a.id));
      if (!pending) return;
      const v = useVoiceStore.getState();
      if (!v.replyPending && !v.wakeActive) return; // typed request at the keyboard: buttons only
      asked.add(pending.id);
      void (async () => {
        const what = pending.description.charAt(0).toLowerCase() + pending.description.slice(1);
        const answer = await useVoiceStore.getState().ask(`I need your approval to ${what}. Say yes or no.`);
        const stillWaiting = useChatStore.getState().streaming?.activities.some((a) => a.id === pending.id && a.status === "awaitingApproval");
        if (!stillWaiting) return; // answered with the buttons meanwhile
        const yes = answer === null ? null : parseYesNo(answer);
        if (yes === null) {
          await useVoiceStore.getState().say("I didn't catch that. Use the buttons on screen to allow or deny.");
          return;
        }
        await useChatStore.getState().answerApproval(pending.id, yes);
      })();
    });
  }, []);
}
