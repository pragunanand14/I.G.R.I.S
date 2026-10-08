import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { CommandPalette } from "./CommandPalette";
import { TopBar } from "./TopBar";

function Where() {
  return <p data-testid="where">{useLocation().pathname}</p>;
}

describe("desktop top bar", () => {
  it("links every page and Settings, with the current page marked", () => {
    render(
      <MemoryRouter initialEntries={["/tasks"]}>
        <TopBar onCommand={() => undefined} />
      </MemoryRouter>,
    );
    for (const item of NAV_ITEMS) expect(screen.getByRole("link", { name: item.label })).toHaveAttribute("href", item.path);
    expect(screen.getByRole("link", { name: "Tasks" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Home" })).not.toHaveAttribute("aria-current");
  });

  it("opens the quick switcher", () => {
    const onCommand = vi.fn();
    render(
      <MemoryRouter>
        <TopBar onCommand={onCommand} />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByRole("button", { name: /Go to/ }));
    expect(onCommand).toHaveBeenCalled();
  });
});

describe("quick switcher", () => {
  it("filters as you type and goes where Enter says", () => {
    const onClose = vi.fn();
    render(
      <MemoryRouter>
        <CommandPalette open onClose={onClose} />
        <Routes>
          <Route path="*" element={<Where />} />
        </Routes>
      </MemoryRouter>,
    );
    const input = screen.getByRole("textbox", { name: "Go to" });
    fireEvent.change(input, { target: { value: "secur" } });
    expect(screen.getAllByRole("option")).toHaveLength(1);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onClose).toHaveBeenCalled();
    expect(screen.getByTestId("where")).toHaveTextContent("/security");
  });

  it("renders nothing while closed", () => {
    render(
      <MemoryRouter>
        <CommandPalette open={false} onClose={() => undefined} />
      </MemoryRouter>,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
