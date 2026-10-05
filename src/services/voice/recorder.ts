import { DEFAULT_SILENCE, rms, SilenceDetector, type SilenceOptions } from "./silence";

export class MicUnavailableError extends Error {}

export function micSupported(): boolean {
  return typeof navigator !== "undefined" && !!navigator.mediaDevices?.getUserMedia && typeof MediaRecorder !== "undefined";
}

async function openMic(): Promise<MediaStream> {
  if (!micSupported()) throw new MicUnavailableError("This system doesn't provide microphone access to IGRIS.");
  try {
    try {
      return await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } });
    } catch (err) {
      // Some webviews reject processing constraints; fall back to a plain request.
      const name = err instanceof DOMException ? err.name : "";
      if (name === "OverconstrainedError" || name === "TypeError" || (err instanceof Error && /constraint/i.test(err.message))) {
        return await navigator.mediaDevices.getUserMedia({ audio: true });
      }
      throw err;
    }
  } catch (err) {
    const name = err instanceof DOMException ? err.name : "";
    if (name === "NotAllowedError" || name === "SecurityError") throw new MicUnavailableError("Microphone permission was denied.");
    if (name === "NotFoundError") throw new MicUnavailableError("No microphone was found.");
    throw new MicUnavailableError(`Couldn't open the microphone${err instanceof Error ? `: ${err.message}` : "."}`);
  }
}

/** Continuously reports microphone level (0–1) until stopped. */
export class LevelMonitor {
  private ctx: AudioContext | null = null;
  private raf = 0;
  private stopped = false;

  constructor(
    private readonly stream: MediaStream,
    private readonly onLevel: (level: number) => void,
  ) {}

  start() {
    this.ctx = new AudioContext();
    const analyser = this.ctx.createAnalyser();
    analyser.fftSize = 1024;
    this.ctx.createMediaStreamSource(this.stream).connect(analyser);
    const buf = new Uint8Array(analyser.fftSize);
    const tick = () => {
      if (this.stopped) return;
      analyser.getByteTimeDomainData(buf);
      this.onLevel(rms(buf));
      this.raf = requestAnimationFrame(tick);
    };
    tick();
  }

  stop() {
    this.stopped = true;
    cancelAnimationFrame(this.raf);
    void this.ctx?.close().catch(() => undefined);
  }
}

export interface Recording {
  blob: Blob;
  mimeType: string;
  /** False when nothing above the speech threshold was heard. */
  heardSpeech: boolean;
}

/** Records one utterance; stops on silence, on `stop()`, or at the length cap. */
export class UtteranceRecorder {
  private stream: MediaStream | null = null;
  private recorder: MediaRecorder | null = null;
  private monitor: LevelMonitor | null = null;
  private cancelled = false;

  constructor(
    private readonly onLevel: (level: number) => void = () => undefined,
    private readonly opts: SilenceOptions = DEFAULT_SILENCE,
  ) {}

  async record(): Promise<Recording | null> {
    this.stream = await openMic();
    const mimeType = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus", "audio/mp4"].find((t) => MediaRecorder.isTypeSupported?.(t)) ?? "";
    const recorder = new MediaRecorder(this.stream, mimeType ? { mimeType } : undefined);
    this.recorder = recorder;
    const chunks: Blob[] = [];
    recorder.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);
    const detector = new SilenceDetector(this.opts);
    let heardSpeech = false;

    const done = new Promise<void>((resolve) => (recorder.onstop = () => resolve()));
    this.monitor = new LevelMonitor(this.stream, (level) => {
      this.onLevel(level);
      const state = detector.update(level, performance.now());
      if (state === "speech") heardSpeech = true;
      if ((state === "done" || state === "timeout") && recorder.state === "recording") recorder.stop();
    });
    recorder.start(250);
    this.monitor.start();
    await done;
    this.cleanup();
    if (this.cancelled) return null;
    const type = recorder.mimeType || mimeType || "audio/webm";
    return { blob: new Blob(chunks, { type }), mimeType: type, heardSpeech };
  }

  /** Finish now and keep what was recorded. */
  stop() {
    if (this.recorder?.state === "recording") this.recorder.stop();
  }

  /** Abort and discard. */
  cancel() {
    this.cancelled = true;
    this.stop();
    this.cleanup();
  }

  private cleanup() {
    this.monitor?.stop();
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null;
  }
}

/** Watches the microphone while IGRIS speaks; calls `onSpeech` when the user starts talking. */
export async function watchForBargeIn(onSpeech: () => void, threshold = 0.06, sustainMs = 300): Promise<() => void> {
  const stream = await openMic();
  let since: number | null = null;
  let fired = false;
  const monitor = new LevelMonitor(stream, (level) => {
    const now = performance.now();
    if (level >= threshold) {
      since ??= now;
      if (!fired && now - since >= sustainMs) {
        fired = true;
        onSpeech();
      }
    } else {
      since = null;
    }
  });
  monitor.start();
  return () => {
    monitor.stop();
    stream.getTracks().forEach((t) => t.stop());
  };
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}
