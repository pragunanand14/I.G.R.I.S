import { render, screen } from "@testing-library/react";
import { CheckSquare } from "lucide-react";
import { MemoryRouter } from "react-router";
import type { NavItem } from "@/config/navigation";
import { PlannedPage } from "./PlannedPage";

describe("PlannedPage", () => {
  it("clearly states the feature is not available", () => {
    const item: NavItem = { path: "/future", label: "Future", icon: CheckSquare, plannedPhase: 11, summary: "Not built yet.", planned: ["Something"] };
    render(<MemoryRouter><PlannedPage item={item} /></MemoryRouter>);
    expect(screen.getByText(/Not available yet/)).toBeInTheDocument();
    expect(screen.getByText(/Phase 11/)).toBeInTheDocument();
  });
});
