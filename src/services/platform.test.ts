import { describe, expect, it } from "vitest";
import { isMobilePlatform } from "./platform";

describe("isMobilePlatform", () => {
  it("recognises the Android webview", () => {
    expect(isMobilePlatform("Mozilla/5.0 (Linux; Android 14; sdk_gphone64_x86_64 Build/UE1A; wv) AppleWebKit/537.36 Chrome/124.0 Mobile Safari/537.36")).toBe(true);
  });

  it("treats desktop webviews as desktop", () => {
    expect(isMobilePlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0 Safari/537.36 Edg/131.0")).toBe(false);
    expect(isMobilePlatform("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)")).toBe(false);
  });
});
