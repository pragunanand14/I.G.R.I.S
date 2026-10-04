import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { useAppStore } from "@/stores/appStore";
import { HomePage } from "./HomePage";

describe("HomePage without a backend", () => {
  beforeEach(async () => {
    useAppStore.setState({ backend: "connecting", backendError: null, info: null, config: null });
    await useAppStore.getState().init();
  });

  it("reports offline and shows the error core state", () => {
    render(<MemoryRouter><HomePage /></MemoryRouter>);
    expect(screen.getByText("Offline")).toBeInTheDocument();
    expect(screen.getByText("Attention required")).toBeInTheDocument();
  });

  it("does not offer a fake chat input", () => {
    render(<MemoryRouter><HomePage /></MemoryRouter>);
    expect(screen.getByLabelText("Message IGRIS")).toBeDisabled();
    expect(screen.getByLabelText("Send")).toBeDisabled();
  });

  it("shows dashes rather than invented telemetry", () => {
    render(<MemoryRouter><HomePage /></MemoryRouter>);
    expect(screen.getAllByText("—").length).toBeGreaterThanOrEqual(4);
  });
});
