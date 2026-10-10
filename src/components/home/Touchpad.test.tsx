import { act, fireEvent, render, screen } from "@testing-library/react";
import { Touchpad } from "./Touchpad";

const api = vi.hoisted(() => ({ homePointer: vi.fn(), homePointerStop: vi.fn() }));
vi.mock("@/services/api", () => ({ api }));

// jsdom has no PointerEvent or pointer capture.
class FakePointerEvent extends MouseEvent {
  pointerId: number;
  pointerType: string;
  constructor(type: string, init: MouseEventInit & { pointerId?: number; pointerType?: string } = {}) {
    super(type, { bubbles: true, ...init });
    this.pointerId = init.pointerId ?? 1;
    this.pointerType = init.pointerType ?? "touch";
  }
}
const pointer = (el: Element, type: string, x: number, y: number, id = 1) =>
  fireEvent(el, new FakePointerEvent(type, { clientX: x, clientY: y, pointerId: id, pointerType: "touch" }));

describe("TV touchpad", () => {
  beforeAll(() => {
    HTMLElement.prototype.setPointerCapture = () => undefined;
  });
  beforeEach(() => {
    vi.useFakeTimers();
    api.homePointer.mockReset().mockResolvedValue(undefined);
    api.homePointerStop.mockReset().mockResolvedValue(undefined);
  });
  afterEach(() => vi.useRealTimers());

  const sent = () => api.homePointer.mock.calls.flatMap((c) => c[1]);

  it("a finger slide moves the cursor (faster than the finger), a tap clicks", async () => {
    render(<Touchpad id="tv1" phone />);
    const pad = screen.getByRole("application", { name: "TV touchpad" });
    pointer(pad, "pointerdown", 100, 100);
    pointer(pad, "pointermove", 110, 105);
    pointer(pad, "pointermove", 120, 110);
    pointer(pad, "pointerup", 120, 110);
    await act(async () => vi.advanceTimersByTime(100));
    const moves = sent().filter((e) => e.type === "move");
    expect(moves.reduce((s, e) => s + e.dx, 0)).toBe(44); // 20 px × 2.2
    expect(moves.reduce((s, e) => s + e.dy, 0)).toBe(22);
    expect(sent().some((e) => e.type === "click")).toBe(false);

    pointer(pad, "pointerdown", 50, 50);
    pointer(pad, "pointerup", 50, 50);
    await act(async () => vi.advanceTimersByTime(100));
    expect(sent().filter((e) => e.type === "click")).toHaveLength(1);
  });

  it("two fingers scroll instead of moving", async () => {
    render(<Touchpad id="tv1" phone />);
    const pad = screen.getByRole("application", { name: "TV touchpad" });
    pointer(pad, "pointerdown", 100, 100, 1);
    pointer(pad, "pointerdown", 140, 100, 2);
    pointer(pad, "pointermove", 100, 60, 1);
    pointer(pad, "pointermove", 140, 60, 2);
    pointer(pad, "pointerup", 100, 60, 1);
    pointer(pad, "pointerup", 140, 60, 2);
    await act(async () => vi.advanceTimersByTime(100));
    expect(sent().some((e) => e.type === "move")).toBe(false);
    expect(sent().filter((e) => e.type === "scroll").reduce((s, e) => s + e.dy, 0)).toBe(40);
    expect(sent().some((e) => e.type === "click")).toBe(false);
  });

  it("releases the TV's input channel when it goes away", () => {
    const { unmount } = render(<Touchpad id="tv1" />);
    expect(screen.getByRole("button", { name: /Use your touchpad/ })).toBeInTheDocument();
    unmount();
    expect(api.homePointerStop).toHaveBeenCalledWith("tv1");
  });
});
