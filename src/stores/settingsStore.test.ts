import { vi } from "vitest";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import { DEFAULT_SETTINGS, useSettingsStore } from "./settingsStore";

describe("settings store", () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: DEFAULT_SETTINGS, status: "idle", error: null, saving: false });
    vi.restoreAllMocks();
  });

  it("only reflects backend-confirmed settings", async () => {
    vi.spyOn(api, "updateSettings").mockResolvedValue({ ...DEFAULT_SETTINGS, accent: "violet" });
    await expect(useSettingsStore.getState().update({ accent: "violet" })).resolves.toBe(true);
    expect(useSettingsStore.getState().settings.accent).toBe("violet");
  });

  it("keeps previous settings and surfaces the error when an update is rejected", async () => {
    vi.spyOn(api, "updateSettings").mockRejectedValue(new BackendError("validation", "Name must be at most 48 characters."));
    await expect(useSettingsStore.getState().update({ userName: "x".repeat(60) })).resolves.toBe(false);
    const s = useSettingsStore.getState();
    expect(s.settings).toEqual(DEFAULT_SETTINGS);
    expect(s.error).toMatch(/at most 48/);
    expect(s.saving).toBe(false);
  });

  it("reports an error status when loading without a backend", async () => {
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState().status).toBe("error");
  });
});
