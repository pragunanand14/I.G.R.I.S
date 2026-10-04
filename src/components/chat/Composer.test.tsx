import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { Composer } from "./Composer";

describe("Composer", () => {
  it("sends on Enter and inserts newlines with Shift+Enter", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<Composer onSend={onSend} onStop={() => undefined} streaming={false} disabled={false} />);
    const box = screen.getByLabelText("Message IGRIS");
    await userEvent.type(box, "line one{Shift>}{Enter}{/Shift}line two{Enter}");
    expect(onSend).toHaveBeenCalledWith("line one\nline two");
    expect(box).toHaveValue("");
  });

  it("restores the draft when sending is rejected", async () => {
    const onSend = vi.fn().mockResolvedValue(false);
    render(<Composer onSend={onSend} onStop={() => undefined} streaming={false} disabled={false} />);
    const box = screen.getByLabelText("Message IGRIS");
    await userEvent.type(box, "keep me{Enter}");
    expect(box).toHaveValue("keep me");
  });

  it("shows a stop button while streaming", async () => {
    const onStop = vi.fn();
    render(<Composer onSend={vi.fn()} onStop={onStop} streaming disabled={false} />);
    await userEvent.click(screen.getByRole("button", { name: "Stop responding" }));
    expect(onStop).toHaveBeenCalled();
  });

  it("explains why it is disabled", () => {
    render(<Composer onSend={vi.fn()} onStop={() => undefined} streaming={false} disabled disabledReason="Configure an AI provider." />);
    expect(screen.getByLabelText("Message IGRIS")).toHaveAttribute("placeholder", "Configure an AI provider.");
  });
});
