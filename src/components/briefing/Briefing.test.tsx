import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import { useAppStore } from "@/stores/appStore";
import { useVoiceStore } from "@/stores/voiceStore";
import type { Briefing } from "@/types/briefing";
import { BriefingCard } from "./BriefingCard";
import { BriefingSettings } from "./BriefingSettings";

vi.mock("@/services/backend", async (orig) => ({ ...(await orig<typeof import("@/services/backend")>()), hasBackend: () => true }));

const off: Briefing = { enabled: false, time: "07:30", weekdaysOnly: false, weather: true, calendar: true, email: true, tasks: true, news: [], routineId: null };

describe("briefing", () => {
  afterEach(() => vi.restoreAllMocks());

  it("turns on with the chosen time, topics and city", async () => {
    vi.spyOn(api, "briefingGet").mockResolvedValue(off);
    vi.spyOn(api, "weatherHome").mockResolvedValue(null);
    const home = vi.spyOn(api, "weatherSetHome").mockResolvedValue({ name: "Mumbai, Maharashtra, India", latitude: 19, longitude: 72.8 });
    const set = vi.spyOn(api, "briefingSet").mockImplementation(async (b) => ({ ...b, routineId: 3 }));
    render(<BriefingSettings />);
    await userEvent.type(await screen.findByPlaceholderText("e.g. cricket, AI, Mumbai"), "cricket, AI");
    await userEvent.type(screen.getByPlaceholderText("e.g. Mumbai"), "Mumbai");
    await userEvent.click(screen.getByRole("checkbox", { name: "Important emails" }));
    await userEvent.click(screen.getByRole("button", { name: "Turn on the briefing" }));
    expect(home).toHaveBeenCalledWith("Mumbai");
    expect(set).toHaveBeenCalledWith(expect.objectContaining({ enabled: true, email: false, news: ["cricket", "AI"] }));
    expect(await screen.findByText(/set for every day at 07:30/)).toBeInTheDocument();
    expect(screen.getByText(/Weather for Mumbai/)).toBeInTheDocument();
  });

  it("shows today's briefing on Home and reads it aloud", async () => {
    useAppStore.setState({ backend: "ready" });
    vi.spyOn(api, "briefingToday").mockResolvedValue({
      id: 1,
      routineId: 3,
      startedAt: "",
      finishedAt: "",
      status: "done",
      summary: "Sunny, 31°. Two meetings: standup at 10, dentist at 4.",
      conversationId: "c",
    });
    const say = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.setState({ say });
    render(<BriefingCard />);
    expect(await screen.findByText(/Two meetings/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Listen to the briefing" }));
    expect(say).toHaveBeenCalledWith("Sunny, 31°. Two meetings: standup at 10, dentist at 4.");
  });
});
