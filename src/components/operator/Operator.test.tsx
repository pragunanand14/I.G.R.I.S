import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { phaseToCore, useOperatorStore } from "@/stores/operatorStore";
import type { OperatorSnapshot, OperatorTask } from "@/types/operator";
import { OperatorBanner } from "./OperatorBanner";
import { OperatorOrb } from "./OperatorOverlay";

const task = (over: Partial<OperatorTask> = {}): OperatorTask => ({
  id: "t1",
  conversationId: "c1",
  objective: "Draft an email to Professor Sharma",
  plan: [],
  state: "executing",
  phase: "executing",
  status: "OPENING OUTLOOK",
  steps: 2,
  retries: 0,
  result: null,
  error: null,
  pauseReason: null,
  display: null,
  ...over,
});

const show = (snapshot: OperatorSnapshot) => useOperatorStore.setState({ snapshot });

describe("operator mode UI", () => {
  afterEach(() => show({ active: false, task: null }));

  it("maps operator phases to IGRIS core states", () => {
    expect(phaseToCore("planning")).toBe("planning");
    expect(phaseToCore("paused")).toBe("waiting");
    expect(phaseToCore("success")).toBe("success");
    expect(phaseToCore("stopped")).toBe("idle");
  });

  it("shows nothing in the main window when IGRIS isn't operating", () => {
    render(<OperatorBanner />);
    expect(screen.queryByTestId("operator-banner")).toBeNull();
  });

  it("makes control obvious and stoppable from the main window", () => {
    const control = vi.fn().mockResolvedValue(undefined);
    useOperatorStore.setState({ control });
    show({ active: true, task: task() });
    render(<OperatorBanner />);
    expect(screen.getByText("IGRIS is operating your computer")).toBeInTheDocument();
    expect(screen.getByText("OPENING OUTLOOK")).toBeInTheDocument();
    expect(screen.getByText(/Press Esc to stop/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Stop/ }));
    expect(control).toHaveBeenCalledWith("stop");
    fireEvent.click(screen.getByRole("button", { name: /Pause/ }));
    expect(control).toHaveBeenCalledWith("pause");
  });

  it("explains a pause and offers resume", () => {
    const control = vi.fn().mockResolvedValue(undefined);
    useOperatorStore.setState({ control });
    show({ active: true, task: task({ state: "paused", phase: "paused", status: "PAUSED", pauseReason: "You switched to another window, so I paused." }) });
    render(<OperatorBanner />);
    expect(screen.getByText("Operator mode paused")).toBeInTheDocument();
    expect(screen.getByText(/switched to another window/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Resume/ }));
    expect(control).toHaveBeenCalledWith("resume");
  });

  it("orb shows the status while working and the outcome briefly afterwards", () => {
    show({ active: true, task: task({ phase: "verifying", status: "CHECKING DRAFT" }) });
    const { rerender } = render(<OperatorOrb />);
    expect(screen.getByText("VERIFYING")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    show({ active: false, task: task({ state: "completed", phase: "success", status: "COMPLETE" }) });
    rerender(<OperatorOrb />);
    expect(screen.getByText("COMPLETE")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });
});
