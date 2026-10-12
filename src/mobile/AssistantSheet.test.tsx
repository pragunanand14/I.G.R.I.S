import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useChatStore } from "@/stores/chatStore";
import { useVoiceStore } from "@/stores/voiceStore";
import { AssistantSheet } from "./AssistantSheet";
import { useSheet } from "./sheet";

const api = vi.hoisted(() => ({ phoneSheetMode: vi.fn(), phoneSheetHide: vi.fn(), phoneUnlock: vi.fn() }));
vi.mock("@/services/api", () => ({ api }));

describe("assistant panel", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    Object.values(api).forEach((f) => f.mockReset().mockResolvedValue(undefined));
    api.phoneUnlock.mockResolvedValue(false);
    useSheet.getState().enter();
    useSheet.setState({ fresh: false });
  });
  afterEach(() => {
    vi.useRealTimers();
    useChatStore.setState({ streaming: null });
    useVoiceStore.setState({ phase: "idle", lastTranscript: null, error: null, replyPending: false });
  });

  it("shows what was said, the answer and approvals, over a see-through page", () => {
    const answerApproval = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.setState({ phase: "idle", lastTranscript: "Text Mom I'm late" });
    useChatStore.setState({
      answerApproval,
      streaming: {
        requestId: "r",
        conversationId: "c",
        text: "Sending it now.",
        phase: "streaming",
        model: null,
        startedAt: 0,
        activities: [
          { id: "t1", tool: "reply_message", title: "Reply", permission: "low", description: "Reply to Mom on WhatsApp: \"I'm late\"", status: "awaitingApproval", result: null, durationMs: null },
        ],
      },
    });
    render(
      <MemoryRouter>
        <AssistantSheet />
      </MemoryRouter>,
    );
    expect(document.documentElement.dataset.sheet).toBe("1");
    expect(api.phoneSheetMode).toHaveBeenCalledWith(true);
    expect(screen.getByText("Text Mom I'm late")).toBeInTheDocument();
    expect(screen.getByText("Sending it now.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(answerApproval).toHaveBeenCalledWith("t1", true);
  });

  it("goes back to the app underneath when done, and Open IGRIS needs an unlock", async () => {
    render(
      <MemoryRouter>
        <AssistantSheet />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open IGRIS" }));
    await act(async () => undefined);
    expect(api.phoneUnlock).toHaveBeenCalled();
    expect(useSheet.getState().open).toBe(true);
    act(() => vi.advanceTimersByTime(4000));
    expect(useSheet.getState().open).toBe(false);
    expect(api.phoneSheetHide).toHaveBeenCalled();
    expect(document.documentElement.dataset.sheet).toBeUndefined();
  });
});
