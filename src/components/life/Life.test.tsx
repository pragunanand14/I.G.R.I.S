import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SpendingView } from "@/types/life";
import { PeopleList } from "./PeopleList";
import { Spending } from "./Spending";

const api = vi.hoisted(() => ({
  peopleList: vi.fn(),
  personSave: vi.fn(),
  personForget: vi.fn(),
  spendingView: vi.fn(),
  expenseSetCategory: vi.fn(),
  expenseDelete: vi.fn(),
  expensesScan: vi.fn(),
  expensesSetAuto: vi.fn(),
}));
vi.mock("@/services/api", () => ({ api }));

const mom = { id: 1, name: "Mom", relation: "mother", birthday: "03-04", notes: "Loves orchids" };

const spending: SpendingView = {
  from: "2026-10-01",
  to: "2026-10-11",
  totals: [
    { currency: "INR", category: "Food", amountMinor: 55000, count: 2 },
    { currency: "INR", category: "Transport", amountMinor: 12000, count: 1 },
  ],
  items: [{ id: 7, amountMinor: 25000, currency: "INR", merchant: "swiggy", category: "Food", spentOn: "2026-10-10", source: "phone" }],
  auto: false,
  phone: true,
  email: false,
};

describe("life", () => {
  beforeEach(() => Object.values(api).forEach((f) => f.mockReset()));

  it("lists people with upcoming birthdays and adds someone", async () => {
    api.peopleList.mockResolvedValue({ people: [mom], upcoming: [{ person: mom, inDays: 1, turns: null }] });
    api.personSave.mockResolvedValue(mom);
    render(<PeopleList />);
    expect(await screen.findByText("Mom — tomorrow")).toBeInTheDocument();
    expect(screen.getByText("Loves orchids")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Add someone/ }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Priya" } });
    fireEvent.change(screen.getByLabelText("Birthday"), { target: { value: "03-12" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.personSave).toHaveBeenCalledWith({ name: "Priya", relation: "", birthday: "03-12", notes: "" }));
  });

  it("shows totals by category, scans, and fixes a payment", async () => {
    api.spendingView.mockResolvedValue(spending);
    api.expensesScan.mockResolvedValue({ added: 2, notes: [] });
    api.expenseSetCategory.mockResolvedValue({});
    render(<Spending />);
    expect(await screen.findByText("₹670.00")).toBeInTheDocument();
    expect(screen.getByText("₹550.00")).toBeInTheDocument();
    expect(screen.getByText(/payment messages on this phone/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Look for payments now/ }));
    expect(await screen.findByText("Found 2 new payments.")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Category of swiggy"), { target: { value: "Other" } });
    expect(api.expenseSetCategory).toHaveBeenCalledWith(7, "Other");
    fireEvent.click(screen.getByRole("button", { name: "Last month" }));
    await waitFor(() => expect(api.spendingView).toHaveBeenLastCalledWith("last_month"));
  });
});
