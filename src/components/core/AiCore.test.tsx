import { render } from "@testing-library/react";
import type { CoreState } from "@/types/assistant";
import { AiCore } from "./AiCore";

describe("AiCore", () => {
  it.each<CoreState>(["idle", "listening", "thinking", "speaking", "executing", "error"])("renders the %s state", (state) => {
    const { container } = render(<AiCore state={state} />);
    const root = container.firstElementChild!;
    expect(root).toHaveAttribute("data-state", state);
    expect(root).toHaveClass(`ai-core--${state}`);
  });
});
