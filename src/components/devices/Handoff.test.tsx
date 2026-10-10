import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { api } from "@/services/api";
import { useDeviceStore } from "@/stores/deviceStore";
import type { DevicesOverview } from "@/types/devices";
import { HandoffInbox } from "./HandoffInbox";
import { SendToDevice } from "./SendToDevice";

const pc = { deviceId: "pc1", name: "My PC", platform: "windows", trusted: true, revokedAt: null } as unknown as DevicesOverview["devices"][number];

describe("handoff", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    useDeviceStore.setState({ handoffs: [], overview: null });
  });

  it("sends a lone web link as a link and anything else as text", async () => {
    useDeviceStore.setState({ overview: { devices: [pc] } as unknown as DevicesOverview });
    vi.spyOn(api, "devicesOverview").mockResolvedValue({ devices: [pc] } as unknown as DevicesOverview);
    const send = vi.spyOn(api, "devicesHandoff").mockResolvedValue(true);
    render(<SendToDevice />);
    const box = screen.getByRole("textbox", { name: "Send to My PC" });
    await userEvent.type(box, "https://example.com/recipe");
    await userEvent.click(screen.getByRole("button", { name: /Send to My PC/ }));
    expect(send).toHaveBeenCalledWith("pc1", "link", "https://example.com/recipe");
    expect(await screen.findByText("Sent to My PC.")).toBeInTheDocument();
    await userEvent.type(box, "Flat 4B, https://maps.example");
    await userEvent.click(screen.getByRole("button", { name: /Send to My PC/ }));
    expect(send).toHaveBeenLastCalledWith("pc1", "text", "Flat 4B, https://maps.example");
  });

  it("shows received items with Open only for web links, and dismisses them", async () => {
    const dismiss = vi.spyOn(api, "handoffDismiss").mockResolvedValue();
    useDeviceStore.setState({
      handoffs: [
        { id: "h1", fromDevice: "p", fromName: "Pixel", kind: "link", content: "https://example.com", receivedAt: "" },
        { id: "h2", fromDevice: "p", fromName: "Pixel", kind: "text", content: "Flat 4B", receivedAt: "" },
      ],
    });
    render(<HandoffInbox />);
    expect(screen.getAllByText("From Pixel")).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: /Copy/ })).toHaveLength(2);
    await userEvent.click(screen.getAllByRole("button", { name: "Dismiss" })[1]!);
    expect(dismiss).toHaveBeenCalledWith("h2");
    expect(screen.queryByText("Flat 4B")).toBeNull();
  });
});
