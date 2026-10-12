import { api } from "@/services/api";
import type { Recording } from "./recorder";
import { DEFAULT_SILENCE, type SilenceOptions } from "./silence";

function base64ToBlob(b64: string, type: string): Blob {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Blob([bytes], { type });
}

/**
 * The phone's recorder: Android records natively as voice recognition, so the
 * phone doesn't switch to call mode (which turned the music down). Same rules
 * as `UtteranceRecorder` (passed to the phone): stops on a pause after speech,
 * on `stop()`, or at the length cap.
 */
export class NativeRecorder {
  private cancelled = false;
  private stopped = false;

  constructor(
    private readonly onLevel: (level: number) => void = () => undefined,
    private readonly opts: SilenceOptions = DEFAULT_SILENCE,
  ) {}

  async record(): Promise<(Recording & { base64: string }) | null> {
    await api.phoneRecordStart({
      threshold: this.opts.threshold,
      silenceMs: this.opts.silenceMs,
      noSpeechMs: this.opts.noSpeechMs,
      maxMs: this.opts.maxMs,
    });
    // Stopped or cancelled while the phone was still starting (e.g. the permission prompt was up).
    if (this.cancelled) {
      await api.phoneRecordStop(true).catch(() => undefined);
      return null;
    }
    if (this.stopped) await api.phoneRecordStop(false).catch(() => undefined);
    for (;;) {
      await new Promise((r) => setTimeout(r, 100));
      const p = await api.phoneRecordPoll();
      this.onLevel(p.level);
      if (p.state === "failed") throw new Error(p.error ?? "Couldn't record.");
      if (p.state === "recording") continue;
      if (this.cancelled || p.state === "idle") return null;
      // Nothing was said: no need to fetch the silence.
      if (!p.heardSpeech)
        return {
          blob: new Blob([], { type: "audio/wav" }),
          base64: "",
          mimeType: "audio/wav",
          heardSpeech: false,
        };
      const wav = await api.phoneRecordTake();
      if (!wav) return null;
      return {
        blob: base64ToBlob(wav, "audio/wav"),
        base64: wav,
        mimeType: "audio/wav",
        heardSpeech: true,
      };
    }
  }

  stop() {
    this.stopped = true;
    void api.phoneRecordStop(false).catch(() => undefined);
  }

  cancel() {
    this.cancelled = true;
    this.stopped = true;
    void api.phoneRecordStop(true).catch(() => undefined);
  }
}
