import { addDays, dayDiff, formatCountdown, formatDay, formatRelative, fromInputs, toDateInput, toTimeInput } from "./time";

describe("time utils", () => {
  const now = new Date(2026, 9, 5, 10, 30); // Mon 5 Oct 2026, local

  it("names nearby days", () => {
    expect(formatDay(now, now)).toBe("Today");
    expect(formatDay(addDays(now, 1), now)).toBe("Tomorrow");
    expect(formatDay(addDays(now, -1), now)).toBe("Yesterday");
    expect(dayDiff(new Date(2026, 9, 12, 23, 59), now)).toBe(7);
  });

  it("formats countdowns and relative times", () => {
    expect(formatCountdown(245_000)).toBe("4:05");
    expect(formatCountdown(3_723_000)).toBe("1:02:03");
    expect(formatCountdown(-5)).toBe("0:00");
    expect(formatCountdown(1)).toBe("0:01"); // rounds up so it never shows 0:00 early
    expect(formatRelative(5 * 60_000)).toBe("in 5 min");
    expect(formatRelative(130 * 60_000)).toBe("in 2 h 10 min");
    expect(formatRelative(-3 * 86_400_000)).toBe("3 days ago");
    expect(formatRelative(10_000)).toBe("in under a minute");
  });

  it("round-trips date/time inputs through UTC", () => {
    const iso = fromInputs("2026-10-06", "17:45");
    const d = new Date(iso);
    expect(toDateInput(d)).toBe("2026-10-06");
    expect(toTimeInput(d)).toBe("17:45");
    expect(fromInputs("", "10:00")).toBe("");
    expect(fromInputs("2026-10-06", "xx")).toBe("");
  });
});
