import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import type { ToolActivity } from "@/types/tools";
import { ToolActivityList } from "./ToolActivityList";

const base: ToolActivity = {
  id: "t1",
  tool: "launch_application",
  title: "Open application",
  permission: "low",
  description: "Open VS Code",
  status: "awaitingApproval",
  result: null,
  durationMs: null,
};

describe("ToolActivityList", () => {
  it("asks for approval with allow and deny", async () => {
    const onAnswer = vi.fn();
    render(<ToolActivityList activities={[base]} onAnswer={onAnswer} />);
    expect(screen.getByRole("alertdialog", { name: "Approve: Open VS Code" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(onAnswer).toHaveBeenCalledWith("t1", false);
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(onAnswer).toHaveBeenCalledWith("t1", true);
  });

  it("never offers approval buttons on stored history", () => {
    render(<ToolActivityList activities={[base]} />);
    expect(screen.queryByRole("button", { name: "Allow" })).toBeNull();
  });

  it("shows results and honest failure states", () => {
    render(
      <ToolActivityList
        activities={[
          { ...base, id: "a", tool: "calculator", description: "Calculate 6*7", status: "completed", result: "= 42", durationMs: 2 },
          { ...base, id: "b", status: "failed", result: "VS Code started but exited immediately (exit code 1)." },
          { ...base, id: "c", status: "denied", result: "Denied" },
        ]}
      />,
    );
    expect(screen.getByText("— = 42")).toBeInTheDocument();
    expect(screen.getByText(/exited immediately/)).toBeInTheDocument();
    expect(screen.getByText("— Denied")).toBeInTheDocument();
    expect(screen.getByText("2 ms")).toBeInTheDocument();
  });
});
