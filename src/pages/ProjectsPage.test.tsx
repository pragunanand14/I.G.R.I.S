import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useAppStore } from "@/stores/appStore";
import { toInput } from "@/types/workspace";
import { ProjectsPage } from "./ProjectsPage";

describe("ProjectsPage", () => {
  it("explains that the backend is required", () => {
    useAppStore.setState({ backend: "unavailable" });
    render(<MemoryRouter><ProjectsPage /></MemoryRouter>);
    expect(screen.getByText(/Projects require the IGRIS desktop backend/)).toBeInTheDocument();
  });

  it("sends only editable fields when updating", () => {
    const input = toInput({
      id: 3, name: "SkillTrack", path: "/p", repository: "", language: "Java", framework: "Spring",
      description: "", notes: "", createdAt: "x", updatedAt: "y",
    });
    expect(Object.keys(input).sort()).toEqual(["description", "framework", "language", "name", "notes", "path", "repository"]);
  });
});
