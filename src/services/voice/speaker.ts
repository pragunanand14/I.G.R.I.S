import { api } from "@/services/api";

export type SpeakerMode = "browser" | "backend";

export function browserTtsAvailable(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window && typeof SpeechSynthesisUtterance !== "undefined";
}

export function browserVoices(): SpeechSynthesisVoice[] {
  return browserTtsAvailable() ? window.speechSynthesis.getVoices() : [];
}

/**
 * The chosen voice, or — when none is chosen — the most natural one available:
 * Windows' neural "Natural"/"Online" voices sound far less robotic than the
 * classic SAPI ones.
 */
export function pickVoice(voices: SpeechSynthesisVoice[], name: string): SpeechSynthesisVoice | undefined {
  if (name) return voices.find((v) => v.name === name);
  const lang = (typeof navigator !== "undefined" ? navigator.language : "en").slice(0, 2).toLowerCase();
  const sameLang = voices.filter((v) => v.lang.toLowerCase().startsWith(lang));
  const pool = sameLang.length ? sameLang : voices;
  return pool.find((v) => /natural/i.test(v.name)) ?? pool.find((v) => /online/i.test(v.name)) ?? pool.find((v) => v.default) ?? pool[0];
}

/** Synthesized audio arrives as MP3 or WAV depending on the service. */
export function audioMime(b64: string): string {
  if (b64.startsWith("UklGR")) return "audio/wav"; // "RIFF"
  if (b64.startsWith("T2dnUw")) return "audio/ogg"; // "OggS"
  return "audio/mpeg";
}

interface Item {
  text: string;
  /** Backend audio, requested ahead of time so sentences follow without gaps. */
  audio?: Promise<string>;
}

/** Sentences synthesized ahead of the one playing. */
const PREFETCH = 2;

/**
 * Speaks queued sentences in order. `stop()` silences immediately and drops
 * the queue (used for interruptions).
 */
export class Speaker {
  private queue: Item[] = [];
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
    this.queue.push(...sentences.filter((s) => s.trim()).map((text) => ({ text })));
    this.prefetch();
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

  private prefetch() {
    if (this.mode !== "backend") return;
    for (const item of this.queue.slice(0, PREFETCH)) {
      if (!item.audio) {
        item.audio = api.synthesizeSpeech(item.text);
        item.audio.catch(() => undefined); // reported when it's this item's turn
      }
    }
  }

  private async pump() {
    if (this.busy || this.stopped) return;
    const next = this.queue.shift();
    if (next === undefined) return this.settle();
    this.busy = true;
    this.onState(true);
    this.prefetch();
    try {
      await (this.mode === "browser" ? this.speakBrowser(next.text) : this.speakBackend(next));
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
      const voice = pickVoice(browserVoices(), this.voiceName);
      if (voice) u.voice = voice;
      u.onend = () => resolve();
      u.onerror = (e) => (e.error === "interrupted" || e.error === "canceled" ? resolve() : reject(new Error(`Speech failed (${e.error}).`)));
      window.speechSynthesis.speak(u);
    });
  }

  private async speakBackend(item: Item): Promise<void> {
    const b64 = await (item.audio ?? api.synthesizeSpeech(item.text));
    if (this.stopped) return;
    const audio = new Audio(`data:${audioMime(b64)};base64,${b64}`);
    this.audio = audio;
    await new Promise<void>((resolve, reject) => {
      audio.onended = () => resolve();
      audio.onerror = () => reject(new Error("Couldn't play the synthesized speech."));
      audio.play().catch(reject);
    });
  }
}
