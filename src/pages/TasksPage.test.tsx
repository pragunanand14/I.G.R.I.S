import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useAppStore } from "@/stores/appStore";
import { TasksPage } from "./TasksPage";

describe("TasksPage", () => {
  it("explains that the backend is required", () => {
    useAppStore.setState({ backend: "unavailable" });
    render(<MemoryRouter><TasksPage /></MemoryRouter>);
    expect(screen.getByText(/require the IGRIS desktop backend/)).toBeInTheDocument();
  });
});
