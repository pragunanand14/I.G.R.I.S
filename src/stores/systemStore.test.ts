import { vi } from "vitest";
import { api } from "@/services/api";
import type { SystemSnapshot } from "@/types/system";
import { HISTORY_LENGTH, useSystemStore } from "./systemStore";

function snap(cpu: number | null): SystemSnapshot {
  return {
    timestampMs: 0,
    host: { hostname: "h", osName: "os", osVersion: "1", kernelVersion: "k", arch: "x86_64" },
    uptimeSecs: 1,
    cpu: { usagePercent: cpu, brand: "cpu", logicalCores: 8, physicalCores: 4, frequencyMhz: null },
    memory: { totalBytes: 100, usedBytes: 25, availableBytes: 75, swapTotalBytes: 0, swapUsedBytes: 0 },
    disks: [],
    network: { connectivity: "online", rxBytesPerSec: null, txBytesPerSec: null, interfaceCount: 1 },
    battery: null,
    temperatures: [],
  };
}

describe("system store", () => {
  beforeEach(() => {
    useSystemStore.setState({ snapshot: null, error: null, cpuHistory: [], memoryHistory: [] });
    vi.restoreAllMocks();
  });

  it("does not record CPU history until a real sample exists", async () => {
    vi.spyOn(api, "getSystemSnapshot").mockResolvedValue(snap(null));
    await useSystemStore.getState().sample();
    expect(useSystemStore.getState().cpuHistory).toEqual([]);
    expect(useSystemStore.getState().memoryHistory).toEqual([25]);
  });

  it("caps history length", async () => {
    const spy = vi.spyOn(api, "getSystemSnapshot");
    for (let i = 0; i < HISTORY_LENGTH + 5; i++) {
      spy.mockResolvedValueOnce(snap(i));
      await useSystemStore.getState().sample();
    }
    const h = useSystemStore.getState().cpuHistory;
    expect(h).toHaveLength(HISTORY_LENGTH);
    expect(h.at(-1)).toBe(HISTORY_LENGTH + 4);
  });

  it("keeps the last snapshot and records the error on failure", async () => {
    vi.spyOn(api, "getSystemSnapshot").mockResolvedValueOnce(snap(10)).mockRejectedValueOnce(new Error("lock poisoned"));
    await useSystemStore.getState().sample();
    await useSystemStore.getState().sample();
    const s = useSystemStore.getState();
    expect(s.snapshot?.cpu.usagePercent).toBe(10);
    expect(s.error).toBe("lock poisoned");
  });
});
