import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { NAV_ITEMS } from "@/config/navigation";
import { PlannedPage } from "./PlannedPage";

describe("PlannedPage", () => {
  it("clearly states the feature is not available", () => {
    const item = NAV_ITEMS.find((n) => n.plannedPhase !== undefined)!;
    render(<MemoryRouter><PlannedPage item={item} /></MemoryRouter>);
    expect(screen.getByText(/Not available yet/)).toBeInTheDocument();
    expect(screen.getByText(new RegExp(`Phase ${item.plannedPhase}`))).toBeInTheDocument();
  });
});
