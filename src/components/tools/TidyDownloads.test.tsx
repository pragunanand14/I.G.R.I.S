import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TidyDownloads } from "./TidyDownloads";

const api = vi.hoisted(() => ({
  tidyStatus: vi.fn(),
  tidyNow: vi.fn(),
  tidyUndo: vi.fn(),
  tidySetAuto: vi.fn(),
}));
vi.mock("@/services/api", () => ({ api }));

describe("tidy downloads", () => {
  it("shows what would move, tidies, and can undo", async () => {
    api.tidyStatus.mockResolvedValueOnce({
      dir: "C:\\Users\\a\\Downloads",
      auto: false,
      pending: "3 files: 2 Documents, 1 Images",
      last: 0,
    });
    api.tidyStatus.mockResolvedValue({
      dir: "C:\\Users\\a\\Downloads",
      auto: false,
      pending: null,
      last: 3,
    });
    api.tidyNow.mockResolvedValue({
      moved: 3,
      summary: "3 files: 2 Documents, 1 Images",
      errors: [],
    });
    api.tidyUndo.mockResolvedValue(3);
    render(<TidyDownloads />);
    expect(
      await screen.findByText("Ready to sort 3 files: 2 Documents, 1 Images."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Tidy now/ }));
    expect(
      await screen.findByText("Moved 3 files: 2 Documents, 1 Images."),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /Undo last tidy/ }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /Undo last tidy/ }));
    expect(await screen.findByText("Put 3 files back.")).toBeInTheDocument();
  });

  it("isn't shown on a device without a Downloads folder", async () => {
    api.tidyStatus.mockResolvedValue({
      dir: null,
      auto: false,
      pending: null,
      last: 0,
    });
    const { container } = render(<TidyDownloads />);
    await waitFor(() => expect(api.tidyStatus).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});
