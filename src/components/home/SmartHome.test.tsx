import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useAppStore } from "@/stores/appStore";
import type { HomeDevice, TvState } from "@/types/home";
import { SmartHome } from "./SmartHome";

const api = vi.hoisted(() => ({
  homeDevices: vi.fn(),
  homeDiscover: vi.fn(),
  homePair: vi.fn(),
  homeRemove: vi.fn(),
  homeTv: vi.fn(),
  homeTvState: vi.fn(),
}));
vi.mock("@/services/api", () => ({ api }));

const tv: HomeDevice = { id: "home_1", kind: "lg_tv", name: "Living room", host: "192.168.1.20", canPowerOn: true, createdAt: "" };
const on: TvState = {
  on: true,
  volume: 12,
  muted: false,
  app: "netflix",
  apps: [
    { id: "netflix", title: "Netflix" },
    { id: "youtube.leanback.v4", title: "YouTube" },
  ],
  inputs: [{ id: "HDMI_1", title: "HDMI 1" }],
};

describe("smart home", () => {
  beforeEach(() => {
    useAppStore.setState({ backend: "ready" });
    Object.values(api).forEach((f) => f.mockReset());
  });
  afterEach(() => useAppStore.setState({ backend: "connecting" }));

  it("adds a TV: finds it, asks you to press Allow on the TV, then shows the remote", async () => {
    api.homeDevices.mockResolvedValueOnce([]).mockResolvedValue([tv]);
    api.homeDiscover.mockResolvedValue([{ kind: "lg_tv", host: "192.168.1.20", name: "Living room" }]);
    let allow: (d: HomeDevice) => void = () => undefined;
    api.homePair.mockReturnValue(new Promise<HomeDevice>((r) => (allow = r)));
    api.homeTvState.mockResolvedValue(on);
    render(<SmartHome />);

    fireEvent.click(await screen.findByRole("button", { name: /Find my TV/ }));
    const found = await screen.findByRole("list", { name: "TVs found" });
    fireEvent.click(within(found).getByRole("button", { name: "Pair" }));
    expect(await screen.findByText("Look at your TV and press Allow.")).toBeInTheDocument();
    expect(api.homePair).toHaveBeenCalledWith("192.168.1.20", "Living room");

    await act(async () => allow(tv));
    expect(await screen.findByRole("region", { name: "Living room remote" })).toBeInTheDocument();
    expect(await screen.findByText(/On · volume 12/)).toBeInTheDocument();
  });

  it("says what the TV did, or why it couldn't", async () => {
    api.homeDevices.mockResolvedValue([tv]);
    api.homeTvState.mockResolvedValue(on);
    api.homeTv.mockResolvedValueOnce("Living room: volume up").mockRejectedValueOnce({ kind: "validation", message: "The TV couldn't do that (No signal)." });
    render(<SmartHome />);
    await screen.findByText(/On · volume 12/);

    fireEvent.click(screen.getByRole("button", { name: "Volume up" }));
    expect(await screen.findByText("Living room: volume up")).toBeInTheDocument();
    expect(api.homeTv).toHaveBeenCalledWith("home_1", "volume_up", undefined);

    await waitFor(() => expect(screen.getByRole("button", { name: "Channel up" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Channel up" }));
    expect(await screen.findByText("The TV couldn't do that (No signal).")).toBeInTheDocument();

    await waitFor(() => expect(screen.getByRole("button", { name: "YouTube" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "YouTube" }));
    await waitFor(() => expect(api.homeTv).toHaveBeenCalledWith("home_1", "open_app", "YouTube"));
  });

  it("an off TV offers only power, and only when it can be switched on remotely", async () => {
    const off: TvState = { on: false, volume: null, muted: null, app: null, apps: [], inputs: [] };
    api.homeDevices.mockResolvedValue([{ ...tv, canPowerOn: false }]);
    api.homeTvState.mockResolvedValue(off);
    render(<SmartHome />);
    expect(await screen.findByText("Off or not reachable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Turn on" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Volume up" })).toBeDisabled();
    expect(screen.getByText(/can't switch this one on yet/)).toBeInTheDocument();
  });

  it("isn't offered without the IGRIS engine", () => {
    useAppStore.setState({ backend: "unavailable" });
    render(<SmartHome />);
    expect(screen.getByText(/available once IGRIS is running/)).toBeInTheDocument();
    expect(api.homeDevices).not.toHaveBeenCalled();
  });
});
