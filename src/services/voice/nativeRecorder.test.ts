import { NativeRecorder } from "./nativeRecorder";

const api = vi.hoisted(() => ({ phoneRecordStart: vi.fn(), phoneRecordPoll: vi.fn(), phoneRecordStop: vi.fn(), phoneRecordTake: vi.fn() }));
vi.mock("@/services/api", () => ({ api }));

describe("phone recorder", () => {
  beforeEach(() => Object.values(api).forEach((f) => f.mockReset().mockResolvedValue(undefined)));

  it("records natively until the phone says it's done, and returns WAV", async () => {
    api.phoneRecordPoll
      .mockResolvedValueOnce({ state: "recording", level: 0.3, heardSpeech: true, error: null })
      .mockResolvedValueOnce({ state: "done", level: 0, heardSpeech: true, error: null });
    api.phoneRecordTake.mockResolvedValue(btoa("RIFFxxxxWAVE"));
    const levels: number[] = [];
    const rec = await new NativeRecorder((l) => levels.push(l)).record();
    expect(api.phoneRecordStart).toHaveBeenCalled();
    expect(rec?.mimeType).toBe("audio/wav");
    expect(rec?.heardSpeech).toBe(true);
    expect(await rec?.blob.text()).toBe("RIFFxxxxWAVE");
    expect(levels).toContain(0.3);
  });

  it("cancelling drops the recording; a failure is reported", async () => {
    api.phoneRecordPoll.mockResolvedValue({ state: "idle", level: 0, heardSpeech: false, error: null });
    const r = new NativeRecorder();
    const p = r.record();
    r.cancel();
    expect(await p).toBeNull();
    expect(api.phoneRecordStop).toHaveBeenCalledWith(true);

    api.phoneRecordPoll.mockResolvedValue({ state: "failed", level: 0, heardSpeech: false, error: "The microphone isn't available." });
    await expect(new NativeRecorder().record()).rejects.toThrow("microphone isn't available");
  });
});
