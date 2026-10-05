export type SilenceState = "waiting" | "speech" | "done" | "timeout";

export interface SilenceOptions {
  /** RMS level (0–1) above which audio counts as speech. */
  threshold: number;
  /** Stop after this much silence following speech. */
  silenceMs: number;
  /** Give up if no speech starts within this time. */
  noSpeechMs: number;
  /** Hard cap on recording length. */
  maxMs: number;
}

export const DEFAULT_SILENCE: SilenceOptions = { threshold: 0.02, silenceMs: 1200, noSpeechMs: 8000, maxMs: 30000 };

/** Pure end-of-utterance detector fed with level samples. */
export class SilenceDetector {
  private start: number | null = null;
  private heardSpeech = false;
  private lastLoud = 0;

  constructor(private readonly opts: SilenceOptions = DEFAULT_SILENCE) {}

  update(level: number, now: number): SilenceState {
    if (this.start === null) this.start = now;
    const elapsed = now - this.start;
    if (level >= this.opts.threshold) {
      this.heardSpeech = true;
      this.lastLoud = now;
    }
    if (elapsed >= this.opts.maxMs) return this.heardSpeech ? "done" : "timeout";
    if (!this.heardSpeech) return elapsed >= this.opts.noSpeechMs ? "timeout" : "waiting";
    return now - this.lastLoud >= this.opts.silenceMs ? "done" : "speech";
  }
}

/** Root-mean-square of time-domain samples centred on 128 (Web Audio byte data). */
export function rms(samples: Uint8Array): number {
  if (samples.length === 0) return 0;
  let sum = 0;
  for (const s of samples) {
    const v = (s - 128) / 128;
    sum += v * v;
  }
  return Math.sqrt(sum / samples.length);
}
