import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useAppStore } from "@/stores/appStore";
import { SecurityPage } from "./SecurityPage";
import { ToolsPage } from "./ToolsPage";

describe("Tools and Security pages without a backend", () => {
  beforeEach(() => useAppStore.setState({ backend: "unavailable" }));

  it("explain that the backend is required instead of showing empty controls", () => {
    render(<MemoryRouter><ToolsPage /></MemoryRouter>);
    expect(screen.getByText(/require the IGRIS desktop backend/)).toBeInTheDocument();
    render(<MemoryRouter><SecurityPage /></MemoryRouter>);
    expect(screen.getByText(/Security settings require/)).toBeInTheDocument();
  });
});
