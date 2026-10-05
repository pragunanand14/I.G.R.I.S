import { create } from "zustand";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { browserRecognitionAvailable, recognizeOnce } from "@/services/voice/browserSpeech";
import { blobToBase64, micSupported, MicUnavailableError, UtteranceRecorder, watchForBargeIn } from "@/services/voice/recorder";
import { Speaker } from "@/services/voice/speaker";
import { SentenceChunker } from "@/services/voice/speechText";
import type { VoicePhase, VoiceStatus } from "@/types/voice";
import { useAssistantStore } from "./assistantStore";
import { useSettingsStore } from "./settingsStore";

type Submit = (text: string) => Promise<boolean>;
type Interrupt = () => Promise<void>;

interface VoiceStore {
  phase: VoicePhase;
  /** Microphone level 0–1 while listening. */
  level: number;
  error: string | null;
  status: VoiceStatus | null;
  lastTranscript: string | null;
  /** The current chat reply was requested by voice and should be spoken. */
  replyPending: boolean;

  loadStatus: () => Promise<void>;
  /** Wire voice into chat (set by the app shell). */
  connect: (submit: Submit, interruptReply: Interrupt) => void;
  /** Mic button / hotkey: start, finish, or interrupt depending on phase. */
  toggle: () => Promise<void>;
  listen: () => Promise<void>;
  /** Abort listening or speaking without sending anything. */
  cancel: () => void;
  /** Speak text directly (used for the voice test). */
  say: (text: string) => Promise<void>;
  /** Called with streamed reply text; speaks complete sentences. */
  replyDelta: (delta: string) => void;
  replyFinished: () => Promise<void>;
  dismissError: () => void;
  /** Whether listening can work at all on this system/configuration. */
  canListen: () => { ok: boolean; reason?: string };
}

let submit: Submit = async () => false;
let interruptReply: Interrupt = async () => undefined;
let recorder: UtteranceRecorder | null = null;
let recognitionCancel: (() => void) | null = null;
let speaker: Speaker | null = null;
let chunker: SentenceChunker | null = null;
let stopBargeIn: (() => void) | null = null;

export const useVoiceStore = create<VoiceStore>((set, get) => {
  const setActivity = (a: Parameters<ReturnType<typeof useAssistantStore.getState>["setActivity"]>[0]) =>
    useAssistantStore.getState().setActivity(a);

  const fail = (msg: string) => {
    set({ phase: "idle", level: 0, error: msg });
    setActivity("idle");
  };

  const stopSpeaking = () => {
    speaker?.stop();
    speaker = null;
    chunker = null;
    stopBargeIn?.();
    stopBargeIn = null;
  };

  const ensureSpeaker = () => {
    if (speaker) return speaker;
    const st = get().status;
    const mode = st && st.tts.mode !== "browser" && !st.tts.problem ? "backend" : "browser";
    speaker = new Speaker(
      mode,
      useSettingsStore.getState().settings.ttsVoice,
      (speaking) => {
        if (speaking) {
          set({ phase: "speaking" });
          setActivity("speaking");
          // Let the user interrupt by talking over IGRIS.
          if (!stopBargeIn && micSupported()) {
            stopBargeIn = () => undefined; // placeholder until the watcher is ready
            watchForBargeIn(() => void get().listen())
              .then((stop) => {
                if (stopBargeIn) stopBargeIn = stop;
                else stop();
              })
              .catch(() => (stopBargeIn = null));
          }
        }
      },
      (msg) => set({ error: msg }),
    );
    return speaker;
  };

  return {
    phase: "idle",
    level: 0,
    error: null,
    status: null,
    lastTranscript: null,
    replyPending: false,

    loadStatus: async () => {
      try {
        set({ status: await api.getVoiceStatus() });
      } catch (err) {
        set({ error: BackendError.from(err).message });
      }
    },

    connect: (s, i) => {
      submit = s;
      interruptReply = i;
    },

    canListen: () => {
      const st = get().status;
      if (st?.stt.problem) return { ok: false, reason: st.stt.problem };
      if (!st || st.stt.mode === "browser") {
        return browserRecognitionAvailable()
          ? { ok: true }
          : { ok: false, reason: "Speech recognition isn't built into this system's webview. Set STT_PROVIDER=openai (or local) in .env." };
      }
      return micSupported() ? { ok: true } : { ok: false, reason: "This system doesn't provide microphone access." };
    },

    toggle: async () => {
      const { phase } = get();
      if (phase === "listening") {
        recorder?.stop();
        recognitionCancel?.();
        return;
      }
      if (phase === "transcribing") return;
      await get().listen();
    },

    listen: async () => {
      // Interrupt anything IGRIS is saying or still generating.
      const wasReplying = get().replyPending || get().phase === "speaking";
      stopSpeaking();
      if (wasReplying) await interruptReply();
      set({ replyPending: false });

      const can = get().canListen();
      if (!can.ok) return fail(can.reason ?? "Voice input isn't available.");

      set({ phase: "listening", level: 0, error: null });
      setActivity("listening");
      let text: string;
      try {
        const st = get().status;
        if (!st || st.stt.mode === "browser") {
          const r = recognizeOnce();
          recognitionCancel = r.cancel;
          text = await r.result;
          recognitionCancel = null;
        } else {
          recorder = new UtteranceRecorder((level) => set({ level }));
          const rec = await recorder.record();
          recorder = null;
          if (!rec) return set({ phase: "idle", level: 0 }), setActivity("idle");
          if (!rec.heardSpeech) return fail("I didn't hear anything.");
          set({ phase: "transcribing", level: 0 });
          setActivity("thinking");
          text = await api.transcribeAudio(await blobToBase64(rec.blob), rec.mimeType);
        }
      } catch (err) {
        recorder = null;
        recognitionCancel = null;
        return fail(err instanceof MicUnavailableError || err instanceof Error ? err.message : BackendError.from(err).message);
      }
      if (get().phase === "idle") return; // cancelled meanwhile
      if (!text.trim()) return fail("I didn't catch that.");
      set({ phase: "idle", level: 0, lastTranscript: text, replyPending: useSettingsStore.getState().settings.voiceAutoSpeak });
      setActivity("thinking");
      const accepted = await submit(text.trim());
      if (!accepted) set({ replyPending: false });
    },

    cancel: () => {
      recorder?.cancel();
      recorder = null;
      recognitionCancel?.();
      recognitionCancel = null;
      stopSpeaking();
      set({ phase: "idle", level: 0, replyPending: false });
      setActivity("idle");
    },

    say: async (text) => {
      stopSpeaking();
      const s = ensureSpeaker();
      s.enqueue([text]);
      await s.drained();
      if (speaker === s) stopSpeaking();
      set({ phase: "idle" });
      setActivity("idle");
    },

    replyDelta: (delta) => {
      if (!get().replyPending) return;
      chunker ??= new SentenceChunker();
      const sentences = chunker.push(delta);
      if (sentences.length) ensureSpeaker().enqueue(sentences);
    },

    replyFinished: async () => {
      if (!get().replyPending) return;
      set({ replyPending: false });
      const rest = chunker?.flush() ?? [];
      chunker = null;
      if (rest.length) ensureSpeaker().enqueue(rest);
      const s = speaker;
      if (s) {
        await s.drained();
        if (speaker === s) stopSpeaking();
      }
      if (get().phase === "speaking") {
        set({ phase: "idle" });
        setActivity("idle");
      }
    },

    dismissError: () => set({ error: null }),
  };
});
