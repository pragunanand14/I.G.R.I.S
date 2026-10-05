import { draftToInput, newDraft, type Draft } from "./eventDraft";

describe("event drafts", () => {
  const base: Draft = { ...newDraft(new Date(2026, 9, 6)), title: "Review" };

  it("converts local inputs to UTC instants", () => {
    const input = draftToInput({ ...base, start: "15:00", end: "16:30" });
    if (typeof input === "string") throw new Error(input);
    expect(new Date(input.endsAt).getTime() - new Date(input.startsAt).getTime()).toBe(90 * 60_000);
    expect(new Date(input.startsAt).getHours()).toBe(15);
  });

  it("rolls an end time before the start into the next day", () => {
    const input = draftToInput({ ...base, start: "22:00", end: "01:00" });
    if (typeof input === "string") throw new Error(input);
    expect(new Date(input.endsAt).getTime() - new Date(input.startsAt).getTime()).toBe(3 * 3_600_000);
  });

  it("drops times for all-day events and reports invalid dates", () => {
    const input = draftToInput({ ...base, allDay: true, start: "15:00", end: "16:00" });
    if (typeof input === "string") throw new Error(input);
    expect(input.endsAt).toBe("");
    expect(new Date(input.startsAt).getHours()).toBe(0);
    expect(typeof draftToInput({ ...base, date: "" })).toBe("string");
  });
});
