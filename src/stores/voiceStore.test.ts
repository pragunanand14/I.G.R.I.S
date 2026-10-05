import { vi } from "vitest";
import { api } from "@/services/api";
import * as browserSpeech from "@/services/voice/browserSpeech";
import * as recorderMod from "@/services/voice/recorder";
import { useAssistantStore } from "./assistantStore";
import { useVoiceStore } from "./voiceStore";

function reset(status: "browser" | "openai") {
  useVoiceStore.setState({
    phase: "idle",
    level: 0,
    error: null,
    lastTranscript: null,
    replyPending: false,
    status: { stt: { mode: status, problem: null }, tts: { mode: "browser", problem: null } },
  });
  useAssistantStore.setState({ activity: "idle" });
}

describe("voice store", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("explains why listening is unavailable instead of failing silently", async () => {
    reset("browser");
    vi.spyOn(browserSpeech, "browserRecognitionAvailable").mockReturnValue(false);
    const can = useVoiceStore.getState().canListen();
    expect(can.ok).toBe(false);
    expect(can.reason).toMatch(/STT_PROVIDER/);
    await useVoiceStore.getState().listen();
    expect(useVoiceStore.getState().error).toMatch(/STT_PROVIDER/);
    expect(useVoiceStore.getState().phase).toBe("idle");
  });

  it("records, transcribes via the backend and submits the text", async () => {
    reset("openai");
    vi.spyOn(recorderMod, "micSupported").mockReturnValue(true);
    vi.spyOn(recorderMod.UtteranceRecorder.prototype, "record").mockResolvedValue({ blob: new Blob(["abc"], { type: "audio/webm" }), mimeType: "audio/webm", heardSpeech: true });
    const transcribe = vi.spyOn(api, "transcribeAudio").mockResolvedValue("Open VS Code");
    const submit = vi.fn().mockResolvedValue(true);
    useVoiceStore.getState().connect(submit, async () => undefined);
    await useVoiceStore.getState().listen();
    expect(transcribe).toHaveBeenCalledWith(btoa("abc"), "audio/webm");
    expect(submit).toHaveBeenCalledWith("Open VS Code");
    const s = useVoiceStore.getState();
    expect(s.lastTranscript).toBe("Open VS Code");
    expect(s.replyPending).toBe(true);
    expect(s.phase).toBe("idle");
  });

  it("reports silence without sending anything", async () => {
    reset("openai");
    vi.spyOn(recorderMod, "micSupported").mockReturnValue(true);
    vi.spyOn(recorderMod.UtteranceRecorder.prototype, "record").mockResolvedValue({ blob: new Blob([]), mimeType: "audio/webm", heardSpeech: false });
    const submit = vi.fn();
    useVoiceStore.getState().connect(submit, async () => undefined);
    await useVoiceStore.getState().listen();
    expect(submit).not.toHaveBeenCalled();
    expect(useVoiceStore.getState().error).toBe("I didn't hear anything.");
  });

  it("interrupts a spoken reply before listening again", async () => {
    reset("openai");
    vi.spyOn(recorderMod, "micSupported").mockReturnValue(true);
    vi.spyOn(recorderMod.UtteranceRecorder.prototype, "record").mockResolvedValue(null);
    const interrupt = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.getState().connect(vi.fn(), interrupt);
    useVoiceStore.setState({ replyPending: true });
    await useVoiceStore.getState().listen();
    expect(interrupt).toHaveBeenCalled();
    expect(useVoiceStore.getState().replyPending).toBe(false);
  });

  it("ignores reply text unless the request was spoken", () => {
    reset("browser");
    useVoiceStore.getState().replyDelta("Hello there. ");
    expect(useVoiceStore.getState().phase).toBe("idle");
  });
});
