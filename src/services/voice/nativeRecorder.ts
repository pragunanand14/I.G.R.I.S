import { api } from "@/services/api";
import type { Recording } from "./recorder";
import { DEFAULT_SILENCE, type SilenceOptions } from "./silence";

/** The phone's recording: WAV as base64 (sent to transcription as it is). */
export type NativeRecording = Recording & { base64: string };

/**
 * The phone's recorder: Android records natively as voice recognition, so the
 * phone doesn't switch to call mode (which turned the music down). Same rules
 * as `UtteranceRecorder` (passed to the phone): stops on a pause after speech,
 * on `stop()`, or at the length cap. Each recording is a session on the
 * phone, so an old recorder can never touch a newer one.
 */
export class NativeRecorder {
  private cancelled = false;
  private stopped = false;
  private session: number | null = null;

  constructor(
    private readonly onLevel: (level: number) => void = () => undefined,
    private readonly opts: SilenceOptions = DEFAULT_SILENCE,
  ) {}

  async record(): Promise<NativeRecording | null> {
    const session = await api.phoneRecordStart({
      threshold: this.opts.threshold,
      silenceMs: this.opts.silenceMs,
      noSpeechMs: this.opts.noSpeechMs,
      maxMs: this.opts.maxMs,
    });
    this.session = session;
    // Stopped or cancelled while the phone was still starting (e.g. the permission prompt was up).
    if (this.cancelled) {
      await api.phoneRecordStop(session, true).catch(() => undefined);
      return null;
    }
    if (this.stopped)
      await api.phoneRecordStop(session, false).catch(() => undefined);
    const empty = new Blob([], { type: "audio/wav" });
    for (;;) {
      await new Promise((r) => setTimeout(r, 100));
      const p = await api.phoneRecordPoll(session);
      this.onLevel(p.level);
      if (p.state === "failed") throw new Error(p.error ?? "Couldn't record.");
      if (p.state === "recording") continue;
      if (this.cancelled || p.state === "idle") return null;
      // Nothing was said: no need to fetch the silence.
      if (!p.heardSpeech)
        return {
          blob: empty,
          base64: "",
          mimeType: "audio/wav",
          heardSpeech: false,
        };
      const wav = await api.phoneRecordTake(session);
      if (!wav) return null;
      return {
        blob: empty,
        base64: wav,
        mimeType: "audio/wav",
        heardSpeech: true,
      };
    }
  }

  stop() {
    this.stopped = true;
    if (this.session !== null)
      void api.phoneRecordStop(this.session, false).catch(() => undefined);
  }

  cancel() {
    this.cancelled = true;
    this.stopped = true;
    if (this.session !== null)
      void api.phoneRecordStop(this.session, true).catch(() => undefined);
  }
}
