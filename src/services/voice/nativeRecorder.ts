import { api } from "@/services/api";
import type { Recording } from "./recorder";

function base64ToBlob(b64: string, type: string): Blob {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Blob([bytes], { type });
}

/**
 * The phone's recorder: Android records natively as voice recognition, so the
 * phone doesn't switch to call mode (which turned the music down). Same
 * behaviour as `UtteranceRecorder`: stops on a pause after speech, on
 * `stop()`, or at the length cap.
 */
export class NativeRecorder {
  private cancelled = false;
  private stopped = false;

  constructor(
    private readonly onLevel: (level: number) => void = () => undefined,
  ) {}

  async record(): Promise<Recording | null> {
    await api.phoneRecordStart();
    for (;;) {
      await new Promise((r) => setTimeout(r, 80));
      const p = await api.phoneRecordPoll();
      this.onLevel(p.level);
      if (p.state === "failed") throw new Error(p.error ?? "Couldn't record.");
      if (p.state === "recording") continue;
      if (this.cancelled || p.state === "idle") return null;
      const wav = await api.phoneRecordTake();
      if (!wav) return null;
      return {
        blob: base64ToBlob(wav, "audio/wav"),
        mimeType: "audio/wav",
        heardSpeech: p.heardSpeech,
      };
    }
  }

  stop() {
    if (this.stopped) return;
    this.stopped = true;
    void api.phoneRecordStop(false).catch(() => undefined);
  }

  cancel() {
    this.cancelled = true;
    this.stopped = true;
    void api.phoneRecordStop(true).catch(() => undefined);
  }
}
