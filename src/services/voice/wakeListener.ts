/**
 * Always-on listening for the wake word.
 *
 * The webview's built-in recogniser doesn't work in WebView2, so wake-word
 * detection is: local voice-activity detection → each spoken phrase is encoded
 * as WAV and transcribed by the configured speech service → IGRIS acts only if
 * the phrase starts with its name. Audio is captured on the audio thread
 * (ScriptProcessor), which keeps running while the window is minimised.
 */
import { openMic } from "./recorder";
import { encodeWav } from "./wav";

export interface SegmenterOptions {
  /** Shortest speech worth transcribing ("IGRIS" alone is ~400 ms). */
  minSpeechMs: number;
  /** Silence that ends a phrase. */
  endSilenceMs: number;
  /** Longest phrase. */
  maxMs: number;
  /** Audio kept from before speech started, so the first syllable isn't cut. */
  prerollMs: number;
  /** Floor for the adaptive speech threshold (RMS 0–1). */
  minThreshold: number;
}

export const SEGMENTER_DEFAULTS: SegmenterOptions = { minSpeechMs: 350, endSilenceMs: 800, maxMs: 12_000, prerollMs: 500, minThreshold: 0.012 };

function rmsOf(frame: Float32Array): number {
  let sum = 0;
  for (const s of frame) sum += s * s;
  return frame.length ? Math.sqrt(sum / frame.length) : 0;
}

function concat(frames: Float32Array[]): Float32Array {
  const out = new Float32Array(frames.reduce((n, f) => n + f.length, 0));
  let o = 0;
  for (const f of frames) {
    out.set(f, o);
    o += f.length;
  }
  return out;
}

/** Splits a stream of audio frames into spoken phrases. Pure; fed frame by frame. */
export class SpeechSegmenter {
  private noise = 0.004;
  private inSpeech = false;
  private loudRun = 0;
  private speechMs = 0;
  private silentMs = 0;
  private pre: Float32Array[] = [];
  private cur: Float32Array[] = [];

  constructor(
    private readonly frameMs: number,
    private readonly o: SegmenterOptions = SEGMENTER_DEFAULTS,
  ) {}

  /** Speech must be clearly above the room's background noise. */
  threshold(): number {
    return Math.max(this.o.minThreshold, this.noise * 3);
  }

  /** Returns a finished phrase's samples, or null. */
  push(frame: Float32Array): Float32Array | null {
    const level = rmsOf(frame);
    const loud = level >= this.threshold();
    if (!this.inSpeech) {
      if (!loud) this.noise = this.noise * 0.97 + level * 0.03;
      this.pre.push(frame);
      const keep = Math.max(1, Math.ceil(this.o.prerollMs / this.frameMs));
      if (this.pre.length > keep) this.pre.shift();
      this.loudRun = loud ? this.loudRun + 1 : 0;
      // Two loud frames in a row (~130 ms) start a phrase; single clicks don't.
      if (this.loudRun >= 2) {
        this.inSpeech = true;
        this.cur = this.pre;
        this.pre = [];
        this.speechMs = this.loudRun * this.frameMs;
        this.silentMs = 0;
      }
      return null;
    }
    this.cur.push(frame);
    if (loud) {
      this.speechMs += this.frameMs;
      this.silentMs = 0;
    } else {
      this.silentMs += this.frameMs;
    }
    if (this.silentMs >= this.o.endSilenceMs || this.cur.length * this.frameMs >= this.o.maxMs) {
      const phrase = this.speechMs >= this.o.minSpeechMs ? concat(this.cur) : null;
      this.reset();
      return phrase;
    }
    return null;
  }

  reset() {
    this.inSpeech = false;
    this.loudRun = 0;
    this.speechMs = 0;
    this.silentMs = 0;
    this.cur = [];
    this.pre = [];
  }
}

export class WakeListener {
  private ctx: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private paused = false;
  private resumeOnGesture = () => void this.ctx?.resume().catch(() => undefined);

  constructor(private readonly onPhrase: (wav: Blob) => void) {}

  async start() {
    this.stream = await openMic();
    try {
      this.ctx = new AudioContext({ sampleRate: 16_000 });
    } catch {
      this.ctx = new AudioContext(); // some engines can't resample; WAV at the native rate is fine
    }
    const ctx = this.ctx;
    const frameSize = 1024;
    const segmenter = new SpeechSegmenter((frameSize / ctx.sampleRate) * 1000);
    const source = ctx.createMediaStreamSource(this.stream);
    // ScriptProcessor runs on the audio thread, so it isn't throttled in the background.
    const node = ctx.createScriptProcessor(frameSize, 1, 1);
    node.onaudioprocess = (e) => {
      if (this.paused) return segmenter.reset();
      const phrase = segmenter.push(new Float32Array(e.inputBuffer.getChannelData(0)));
      if (phrase) this.onPhrase(new Blob([encodeWav(phrase, ctx.sampleRate)], { type: "audio/wav" }));
    };
    const mute = ctx.createGain();
    mute.gain.value = 0;
    source.connect(node);
    node.connect(mute);
    mute.connect(ctx.destination);
    // Audio may start suspended until the first click/keypress (autoplay policy).
    void ctx.resume().catch(() => undefined);
    window.addEventListener("pointerdown", this.resumeOnGesture);
    window.addEventListener("keydown", this.resumeOnGesture);
  }

  /** Stop reacting (e.g. while IGRIS is speaking, so it doesn't hear itself). */
  setPaused(paused: boolean) {
    this.paused = paused;
  }

  stop() {
    window.removeEventListener("pointerdown", this.resumeOnGesture);
    window.removeEventListener("keydown", this.resumeOnGesture);
    void this.ctx?.close().catch(() => undefined);
    this.stream?.getTracks().forEach((t) => t.stop());
    this.ctx = null;
    this.stream = null;
  }
}
