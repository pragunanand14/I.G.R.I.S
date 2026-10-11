import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import type { Routine } from "@/types/routines";
import { RoutinesList } from "./RoutinesList";

const morning: Routine = {
  id: 1,
  name: "Morning",
  instruction: "Tell me the weather and my meetings",
  when: "Every weekday at 08:00",
  enabled: true,
  nextRunAt: null,
  lastRunAt: null,
  createdAt: "2026-10-10T00:00:00Z",
  lastRun: { id: 1, routineId: 1, startedAt: "2026-10-10T02:30:00Z", finishedAt: null, status: "done", summary: "Sunny, 2 meetings.", conversationId: "c1" },
};

describe("RoutinesList", () => {
  afterEach(() => vi.restoreAllMocks());

  it("shows examples when there are none", async () => {
    vi.spyOn(api, "routinesList").mockResolvedValue([]);
    render(<RoutinesList />);
    expect(await screen.findByText(/No routines yet/)).toBeInTheDocument();
  });

  it("shows a routine with its last run and pauses, runs and deletes it", async () => {
    const list = vi.spyOn(api, "routinesList").mockResolvedValue([morning]);
    const toggle = vi.spyOn(api, "routineSetEnabled").mockResolvedValue({});
    const run = vi.spyOn(api, "routineRunNow").mockResolvedValue();
    const del = vi.spyOn(api, "routineDelete").mockResolvedValue();
    render(<RoutinesList />);
    expect(await screen.findByText("Every weekday at 08:00")).toBeInTheDocument();
    expect(screen.getByText(/Sunny, 2 meetings/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("switch", { name: "Morning on" }));
    expect(toggle).toHaveBeenCalledWith(1, false);
    await userEvent.click(screen.getByRole("button", { name: /Run now/ }));
    expect(run).toHaveBeenCalledWith(1);
    await userEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    await userEvent.click(screen.getByRole("button", { name: /Delete “Morning”\?/ }));
    expect(del).toHaveBeenCalledWith(1);
    expect(list.mock.calls.length).toBeGreaterThan(1);
  });
});
