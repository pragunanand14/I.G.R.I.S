import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { MemoryChip } from "./MemoryChip";

describe("MemoryChip", () => {
  it("lists the memories that were sent with a message", async () => {
    render(
      <MemoryRouter>
        <MemoryChip
          context={{
            rendered: "",
            items: [
              { id: 1, kind: "long_term", content: "Main project is SkillTrack", updatedAt: "t" },
              { id: 4, kind: "knowledge", content: "Staging is staging.example.com", updatedAt: "t" },
            ],
          }}
        />
      </MemoryRouter>,
    );
    expect(screen.getByRole("button", { name: /2 memories used/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Main project is SkillTrack")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /2 memories used/ }));
    expect(screen.getByText("Main project is SkillTrack")).toBeInTheDocument();
    expect(screen.getByText("#4")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Manage memory" })).toHaveAttribute("href", "/memory");
  });
});
