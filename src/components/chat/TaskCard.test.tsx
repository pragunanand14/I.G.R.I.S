import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { api } from "@/services/api";
import { latestTask, useTaskStore } from "@/stores/taskStore";
import type { TaskInfo } from "@/types/task";
import { TaskCard } from "./TaskCard";

const task = (over: Partial<TaskInfo> = {}): TaskInfo => ({
  id: "t1",
  conversationId: "c1",
  kind: "general",
  objective: "Create notes.txt and check it",
  state: "executing",
  plan: [],
  currentStep: null,
  activity: "write_file: notes.txt",
  steps: 1,
  failures: 0,
  result: null,
  error: null,
  pauseReason: null,
  context: { actions: [], interrupted: false, resumes: 0 },
  project: null,
  createdAt: "2026-10-06T10:00:00Z",
  updatedAt: "2026-10-06T10:00:00Z",
  live: true,
  ...over,
});

const show = (...tasks: TaskInfo[]) => useTaskStore.setState({ tasks: Object.fromEntries(tasks.map((t) => [t.id, t])), hidden: [] });

describe("task card", () => {
  afterEach(() => useTaskStore.setState({ tasks: {}, hidden: [] }));

  it("shows nothing without a task", () => {
    show();
    render(<TaskCard conversationId="c1" busy={false} onResume={vi.fn()} />);
    expect(screen.queryByTestId("task-card")).toBeNull();
  });

  it("shows real plan progress and live controls", () => {
    const control = vi.fn().mockResolvedValue(undefined);
    useTaskStore.setState({ control });
    show(
      task({
        plan: [
          { title: "Create the file", status: "completed" },
          { title: "Write the sentence", status: "active" },
          { title: "Verify", status: "pending" },
        ],
        currentStep: 1,
      }),
    );
    render(<TaskCard conversationId="c1" busy={false} onResume={vi.fn()} />);
    expect(screen.getByText("Working")).toBeInTheDocument();
    expect(screen.getByText("Step 2 of 3 · Write the sentence")).toBeInTheDocument();
    expect(screen.getByText("write_file: notes.txt")).toBeInTheDocument();
    expect(screen.queryByText(/%/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Pause/ }));
    expect(control).toHaveBeenCalledWith("t1", "pause");
    fireEvent.click(screen.getByRole("button", { name: /Stop/ }));
    expect(control).toHaveBeenCalledWith("t1", "stop");
  });

  it("asks the user before continuing an interrupted task", () => {
    const control = vi.fn().mockResolvedValue(undefined);
    const onResume = vi.fn();
    useTaskStore.setState({ control });
    show(task({ state: "paused", live: false, pauseReason: "IGRIS was closed while this task was running.", context: { actions: [], interrupted: true, resumes: 0 } }));
    render(<TaskCard conversationId="c1" busy={false} onResume={onResume} />);
    expect(screen.getByText("Interrupted")).toBeInTheDocument();
    expect(screen.getByText(/IGRIS was closed/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Pause/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Resume/ }));
    expect(onResume).toHaveBeenCalledWith("t1", "c1");
    fireEvent.click(screen.getByRole("button", { name: /Dismiss/ }));
    expect(control).toHaveBeenCalledWith("t1", "stop");
  });

  it("reports outcomes honestly and can be closed", () => {
    show(task({ state: "ended", live: false, result: "Done, but this couldn't be verified: open_url https://x." }));
    const { rerender, unmount } = render(<TaskCard conversationId="c1" busy={false} onResume={vi.fn()} />);
    expect(screen.getByText("Finished · not verified")).toBeInTheDocument();
    expect(screen.getByText(/couldn't be verified/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    rerender(<TaskCard conversationId="c1" busy={false} onResume={vi.fn()} />);
    expect(screen.queryByTestId("task-card")).toBeNull();
    unmount();

    show(task({ id: "t2", state: "failed", live: false, error: "Not approved: Overwrite notes.txt." }));
    render(<TaskCard conversationId="c1" busy onResume={vi.fn()} />);
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Try again/ })).toBeDisabled();
  });

  it("shows what an interrupted task did, from its activity log", async () => {
    vi.spyOn(api, "listTaskEvents").mockResolvedValue([
      { at: "t", kind: "tool_started", tool: "write_file", detail: "Overwrite notes.txt", state: "executing" },
      { at: "t", kind: "plan_updated", tool: null, detail: "a → b", state: "executing" },
      { at: "t", kind: "interrupted", tool: "write_file", detail: "IGRIS closed while this was running; its result is unknown.", state: "paused" },
    ]);
    show(task({ state: "paused", live: false, context: { actions: [], interrupted: true, resumes: 0 } }));
    render(<TaskCard conversationId="c1" busy={false} onResume={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "What happened" }));
    expect(await screen.findByText(/Started · write_file — Overwrite notes.txt/)).toBeInTheDocument();
    expect(screen.getByText(/Interrupted — IGRIS closed · write_file — IGRIS closed while this was running/)).toBeInTheDocument();
    expect(screen.queryByText(/plan_updated/)).toBeNull();
  });

  it("picks the newest task of the conversation", () => {
    const older = task({ id: "a", createdAt: "2026-10-06T09:00:00Z" });
    const newer = task({ id: "b", createdAt: "2026-10-06T11:00:00Z" });
    const other = task({ id: "c", conversationId: "c2", createdAt: "2026-10-06T12:00:00Z" });
    const all = { a: older, b: newer, c: other };
    expect(latestTask(all, "c1", [])?.id).toBe("b");
    expect(latestTask(all, "c1", ["b"])).toBeNull();
    expect(latestTask(all, null, [])).toBeNull();
  });
});
