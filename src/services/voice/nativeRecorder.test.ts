import { NativeRecorder } from "./nativeRecorder";

const api = vi.hoisted(() => ({
  phoneRecordStart: vi.fn(),
  phoneRecordPoll: vi.fn(),
  phoneRecordStop: vi.fn(),
  phoneRecordTake: vi.fn(),
}));
vi.mock("@/services/api", () => ({ api }));

describe("phone recorder", () => {
  beforeEach(() => {
    Object.values(api).forEach((f) =>
      f.mockReset().mockResolvedValue(undefined),
    );
    api.phoneRecordStart.mockResolvedValue(7);
  });

  it("records natively until the phone says it's done, and returns the WAV as base64", async () => {
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
    expect(api.phoneRecordPoll).toHaveBeenCalledWith(7);
    expect(api.phoneRecordTake).toHaveBeenCalledWith(7);
    expect(rec).toMatchObject({
      base64: btoa("RIFFxxxxWAVE"),
      mimeType: "audio/wav",
      heardSpeech: true,
    });
    expect(levels).toContain(0.3);
  });

  it("cancelling drops the recording; silence isn't fetched; a failure is reported", async () => {
    api.phoneRecordPoll.mockResolvedValue({
      state: "recording",
      level: 0,
      heardSpeech: false,
      error: null,
    });
    const r = new NativeRecorder();
    const p = r.record();
    await new Promise((res) => setTimeout(res, 0));
    r.cancel();
    api.phoneRecordPoll.mockResolvedValue({
      state: "idle",
      level: 0,
      heardSpeech: false,
      error: null,
    });
    expect(await p).toBeNull();
    expect(api.phoneRecordStop).toHaveBeenCalledWith(7, true);

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
    let release = (_: number) => undefined as void;
    api.phoneRecordStart.mockReturnValueOnce(
      new Promise<number>((res) => (release = res)),
    );
    const early = new NativeRecorder();
    const pending = early.record();
    early.cancel();
    release(9);
    expect(await pending).toBeNull();
    expect(api.phoneRecordStop).toHaveBeenCalledWith(9, true);

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
