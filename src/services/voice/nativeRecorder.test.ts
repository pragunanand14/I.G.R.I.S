import { NativeRecorder } from "./nativeRecorder";

const api = vi.hoisted(() => ({
  phoneRecordStart: vi.fn(),
  phoneRecordPoll: vi.fn(),
  phoneRecordStop: vi.fn(),
  phoneRecordTake: vi.fn(),
}));
vi.mock("@/services/api", () => ({ api }));

describe("phone recorder", () => {
  beforeEach(() =>
    Object.values(api).forEach((f) =>
      f.mockReset().mockResolvedValue(undefined),
    ),
  );

  it("records natively until the phone says it's done, and returns WAV", async () => {
    api.phoneRecordPoll
      .mockResolvedValueOnce({
        state: "recording",
        level: 0.3,
        heardSpeech: true,
        error: null,
      })
      .mockResolvedValueOnce({
        state: "done",
        level: 0,
        heardSpeech: true,
        error: null,
      });
    api.phoneRecordTake.mockResolvedValue(btoa("RIFFxxxxWAVE"));
    const levels: number[] = [];
    const rec = await new NativeRecorder((l) => levels.push(l)).record();
    expect(api.phoneRecordStart).toHaveBeenCalledWith({
      threshold: 0.02,
      silenceMs: 1200,
      noSpeechMs: 8000,
      maxMs: 30000,
    });
    expect(rec?.base64).toBe(btoa("RIFFxxxxWAVE"));
    expect(rec?.mimeType).toBe("audio/wav");
    expect(rec?.heardSpeech).toBe(true);
    expect(await rec?.blob.text()).toBe("RIFFxxxxWAVE");
    expect(levels).toContain(0.3);
  });

  it("cancelling drops the recording; a failure is reported", async () => {
    api.phoneRecordPoll.mockResolvedValue({
      state: "idle",
      level: 0,
      heardSpeech: false,
      error: null,
    });
    const r = new NativeRecorder();
    const p = r.record();
    r.cancel();
    expect(await p).toBeNull();
    expect(api.phoneRecordStop).toHaveBeenCalledWith(true);

    // Nothing said: the silence isn't fetched.
    api.phoneRecordPoll.mockResolvedValue({
      state: "done",
      level: 0,
      heardSpeech: false,
      error: null,
    });
    api.phoneRecordTake.mockClear();
    expect((await new NativeRecorder().record())?.heardSpeech).toBe(false);
    expect(api.phoneRecordTake).not.toHaveBeenCalled();

    // Cancelled while the phone was still starting (permission prompt): dropped right away.
    let release = () => undefined as void;
    api.phoneRecordStart.mockReturnValueOnce(
      new Promise<void>((r) => (release = r)),
    );
    const early = new NativeRecorder();
    const pending = early.record();
    early.cancel();
    release();
    expect(await pending).toBeNull();

    api.phoneRecordPoll.mockResolvedValue({
      state: "failed",
      level: 0,
      heardSpeech: false,
      error: "The microphone isn't available.",
    });
    await expect(new NativeRecorder().record()).rejects.toThrow(
      "microphone isn't available",
    );
  });
});
