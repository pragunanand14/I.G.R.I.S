import { formatBytes, formatDuration, formatPercent, formatRate, greetingFor, percentOf } from "./format";

describe("format utils", () => {
  it("formats bytes across units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(16 * 1024 ** 3)).toBe("16.0 GB");
    expect(formatBytes(512 * 1024 ** 3)).toBe("512 GB");
  });

  it("renders a dash for missing or invalid values rather than inventing one", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(-1)).toBe("—");
    expect(formatRate(null)).toBe("—");
    expect(formatPercent(null)).toBe("—");
    expect(formatPercent(Number.NaN)).toBe("—");
    expect(formatDuration(undefined)).toBe("—");
  });

  it("computes bounded percentages", () => {
    expect(percentOf(50, 200)).toBe(25);
    expect(percentOf(300, 200)).toBe(100);
    expect(percentOf(1, 0)).toBeNull();
  });

  it("formats durations", () => {
    expect(formatDuration(42)).toBe("42s");
    expect(formatDuration(125)).toBe("2m");
    expect(formatDuration(3 * 3600 + 120)).toBe("3h 2m");
    expect(formatDuration(2 * 86400 + 5 * 3600)).toBe("2d 5h");
  });

  it("greets by time of day", () => {
    expect(greetingFor(new Date(2026, 0, 1, 9))).toBe("Good morning");
    expect(greetingFor(new Date(2026, 0, 1, 14))).toBe("Good afternoon");
    expect(greetingFor(new Date(2026, 0, 1, 21))).toBe("Good evening");
    expect(greetingFor(new Date(2026, 0, 1, 2))).toBe("Working late");
  });
});
