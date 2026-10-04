import { render, screen } from "@testing-library/react";
import { Markdown } from "./Markdown";

describe("Markdown", () => {
  it("renders formatting, lists and code blocks", () => {
    const { container } = render(<Markdown text={"**bold** and `code`\n\n- one\n- two\n\n```ts\nconst x = 1;\n```"} />);
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    expect(container.querySelectorAll("li")).toHaveLength(2);
    expect(screen.getByText("ts")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
  });

  it("never renders raw HTML from model output", () => {
    const { container } = render(<Markdown text={'<img src=x onerror="alert(1)"><script>alert(1)</script>hello'} />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
  });

  it("does not let links navigate the app window", () => {
    render(<Markdown text="[docs](https://example.com)" />);
    const link = screen.getByRole("link", { name: "docs" });
    const ev = new MouseEvent("click", { bubbles: true, cancelable: true });
    link.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });
});
