import { api } from "@/services/api";

export type SpeakerMode = "browser" | "backend";

export function browserTtsAvailable(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window && typeof SpeechSynthesisUtterance !== "undefined";
}

export function browserVoices(): SpeechSynthesisVoice[] {
  return browserTtsAvailable() ? window.speechSynthesis.getVoices() : [];
}

/**
 * Speaks queued sentences in order. `stop()` silences immediately and drops
 * the queue (used for interruptions).
 */
export class Speaker {
  private queue: string[] = [];
  private busy = false;
  private stopped = false;
  private audio: HTMLAudioElement | null = null;
  private finishedResolvers: (() => void)[] = [];

  constructor(
    private readonly mode: SpeakerMode,
    private readonly voiceName: string,
    private readonly onState: (speaking: boolean) => void,
    private readonly onError: (msg: string) => void,
  ) {}

  enqueue(sentences: string[]) {
    if (this.stopped) return;
    this.queue.push(...sentences.filter((s) => s.trim()));
    void this.pump();
  }

  /** Resolves when everything queued so far has been spoken (or stopped). */
  drained(): Promise<void> {
    if (!this.busy && this.queue.length === 0) return Promise.resolve();
    return new Promise((r) => this.finishedResolvers.push(r));
  }

  stop() {
    this.stopped = true;
    this.queue = [];
    if (browserTtsAvailable()) window.speechSynthesis.cancel();
    this.audio?.pause();
    this.audio = null;
    this.settle();
  }

  private settle() {
    if (this.busy) {
      this.busy = false;
      this.onState(false);
    }
    const r = this.finishedResolvers;
    this.finishedResolvers = [];
    r.forEach((f) => f());
  }

  private async pump() {
    if (this.busy || this.stopped) return;
    const next = this.queue.shift();
    if (next === undefined) return this.settle();
    this.busy = true;
    this.onState(true);
    try {
      await (this.mode === "browser" ? this.speakBrowser(next) : this.speakBackend(next));
    } catch (err) {
      this.onError(err instanceof Error ? err.message : String(err));
      this.queue = [];
    }
    if (this.stopped) return;
    this.busy = false;
    if (this.queue.length) void this.pump();
    else this.settle();
  }

  private speakBrowser(text: string): Promise<void> {
    if (!browserTtsAvailable()) return Promise.reject(new Error("This system has no speech voices available."));
    return new Promise((resolve, reject) => {
      const u = new SpeechSynthesisUtterance(text);
      const voice = browserVoices().find((v) => v.name === this.voiceName);
      if (voice) u.voice = voice;
      u.onend = () => resolve();
      u.onerror = (e) => (e.error === "interrupted" || e.error === "canceled" ? resolve() : reject(new Error(`Speech failed (${e.error}).`)));
      window.speechSynthesis.speak(u);
    });
  }

  private async speakBackend(text: string): Promise<void> {
    const b64 = await api.synthesizeSpeech(text);
    if (this.stopped) return;
    const audio = new Audio(`data:audio/mpeg;base64,${b64}`);
    this.audio = audio;
    await new Promise<void>((resolve, reject) => {
      audio.onended = () => resolve();
      audio.onerror = () => reject(new Error("Couldn't play the synthesized speech."));
      audio.play().catch(reject);
    });
  }
}
