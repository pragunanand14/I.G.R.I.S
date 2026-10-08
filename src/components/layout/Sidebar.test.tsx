import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { Sidebar } from "./Sidebar";

describe("desktop sidebar", () => {
  it("links every page, with the current one marked", () => {
    render(
      <MemoryRouter initialEntries={["/tasks"]}>
        <Sidebar />
      </MemoryRouter>,
    );
    for (const item of NAV_ITEMS) expect(screen.getByRole("link", { name: item.label })).toHaveAttribute("href", item.path);
    expect(screen.getByRole("link", { name: "Tasks" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Home" })).not.toHaveAttribute("aria-current");
  });

  it("says plainly when the engine isn't running", () => {
    render(
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>,
    );
    expect(screen.getByRole("status")).toHaveAttribute("title", expect.stringMatching(/IGRIS core: (Starting|Offline)/));
  });
});
