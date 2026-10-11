import { describe, expect, it } from "vitest";
import { money } from "./life";

describe("money", () => {
  it("shows amounts like the backend does", () => {
    expect(money(12345678, "INR")).toBe("₹123,456.78");
    expect(money(1299, "USD")).toBe("$12.99");
    expect(money(4000, "AED")).toBe("AED 40.00");
  });
});
