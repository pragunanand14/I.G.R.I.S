import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import { useAppStore } from "@/stores/appStore";
import { NowPlayingBar } from "./NowPlayingBar";

vi.mock("@/services/backend", async (orig) => ({ ...(await orig<typeof import("@/services/backend")>()), hasBackend: () => true }));

const song = { playing: true, active: true, title: "Numb", artist: "Linkin Park", app: "Spotify" };

describe("NowPlayingBar", () => {
  beforeEach(() => useAppStore.setState({ backend: "ready" }));
  afterEach(() => vi.restoreAllMocks());

  it("shows what's playing and sends the buttons", async () => {
    vi.spyOn(api, "mediaNowPlaying").mockResolvedValue(song);
    const control = vi.spyOn(api, "mediaControl").mockResolvedValue({ ...song, playing: false });
    render(<NowPlayingBar />);
    expect(await screen.findByText("Numb · Linkin Park")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(control).toHaveBeenCalledWith("play_pause");
    expect(await screen.findByRole("button", { name: "Play" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Next track" }));
    expect(control).toHaveBeenLastCalledWith("next");
  });

  it("stays hidden when nothing plays or music control isn't available", async () => {
    const spy = vi.spyOn(api, "mediaNowPlaying").mockResolvedValue({ playing: false, active: false, title: null, artist: null, app: null });
    const { container, unmount } = render(<NowPlayingBar />);
    await waitFor(() => expect(spy).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
    unmount();
    spy.mockRejectedValue(new Error("not here"));
    const second = render(<NowPlayingBar />);
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));
    expect(second.container).toBeEmptyDOMElement();
  });
});
