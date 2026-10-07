import { fireEvent, render, screen } from "@testing-library/react";
import { useDeviceStore } from "@/stores/deviceStore";
import type { RemoteTask } from "@/types/devices";
import { DevicePrompts } from "./DevicePrompts";
import { linkText, taskStatusText } from "./deviceText";

const task = (status: RemoteTask["status"], direction: RemoteTask["direction"] = "outgoing"): RemoteTask => ({
  requestId: "r".repeat(32),
  direction,
  peerDeviceId: "dev_x",
  peerName: "My PC",
  objective: "Open Notepad",
  conversationId: null,
  taskId: null,
  status,
  detail: null,
  result: null,
  lastSeq: 0,
  createdAt: "",
  updatedAt: "",
});

describe("cross-device wording", () => {
  it("says exactly where a task stands, never 'done' before it is", () => {
    expect(taskStatusText(task("sent"))).toBe("Sent to My PC");
    expect(taskStatusText(task("queued"))).toMatch(/^My PC is offline/);
    expect(taskStatusText(task("accepted"))).toBe("My PC accepted the task");
    expect(taskStatusText(task("completed"))).toBe("Done — completed on My PC");
    expect(taskStatusText(task("timed_out"))).toMatch(/wasn't done/);
    for (const s of ["sent", "queued", "accepted", "running", "waiting_for_approval", "paused", "ended", "failed", "rejected"] as const) {
      expect(taskStatusText(task(s))).not.toMatch(/^Done/);
    }
  });

  it("reports the relay connection honestly", () => {
    expect(linkText({ state: "offline", detail: "The relay didn't answer.", relayUrl: null }).tone).toBe("bad");
    expect(linkText(undefined).text).toBe("Off");
  });
});

describe("device prompts", () => {
  afterEach(() => useDeviceStore.setState({ pairingRequests: [], notices: [], localApprovals: [], overview: null }));

  it("asks before a new device is trusted", () => {
    const confirmPairing = vi.fn(async () => undefined);
    useDeviceStore.setState({ pairingRequests: [{ pairingId: "p1", deviceName: "Pixel", platform: "android" }], confirmPairing, connect: () => () => undefined });
    render(<DevicePrompts enabled />);
    expect(screen.getByText(/Allow "Pixel" \(Android phone\) to join your IGRIS\?/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Don't allow" }));
    expect(confirmPairing).toHaveBeenCalledWith("p1", false);
  });

  it("shows another device's approval request with what will run where", () => {
    const answer = vi.fn(async () => undefined);
    useDeviceStore.setState({
      answer,
      connect: () => () => undefined,
      overview: {
        thisDevice: { deviceId: "d", name: "Pixel", platform: "android", capabilities: [], keyProtection: "android-keystore" },
        settings: { relayUrl: null, enabled: false, syncMemory: true },
        status: { state: "connected", detail: null, relayUrl: null },
        devices: [],
        tasks: [],
        approvals: [
          {
            deviceName: "My PC",
            request: {
              requestId: "r",
              taskId: "t",
              callId: "c1",
              tool: "trash_path",
              title: "Move to Recycle Bin",
              description: "Delete report.docx",
              permission: "critical",
              inputDigest: "x",
              targetDevice: "dev_pc",
              expiresAt: Date.now() + 60_000,
            },
          },
        ],
      },
    });
    render(<DevicePrompts enabled phone />);
    expect(screen.getByText("My PC needs your OK")).toBeInTheDocument();
    expect(screen.getByText("Delete report.docx")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(answer).toHaveBeenCalledWith("c1", true);
  });
});
