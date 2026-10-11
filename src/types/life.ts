export interface Person {
  id: number;
  name: string;
  relation: string;
  /** "MM-DD" or "YYYY-MM-DD"; "" when not known. */
  birthday: string;
  notes: string;
}

export interface UpcomingBirthday {
  person: Person;
  inDays: number;
  turns: number | null;
}

export interface PeopleView {
  people: Person[];
  upcoming: UpcomingBirthday[];
}

export interface Expense {
  id: number;
  amountMinor: number;
  currency: string;
  merchant: string;
  category: string;
  spentOn: string;
  source: "phone" | "email" | "manual";
}

export interface SpendingTotal {
  currency: string;
  category: string;
  amountMinor: number;
  count: number;
}

export interface SpendingView {
  from: string;
  to: string;
  totals: SpendingTotal[];
  items: Expense[];
  auto: boolean;
  phone: boolean;
  email: boolean;
}

export type SpendingPeriod =
  | "today"
  | "this_week"
  | "last_7_days"
  | "this_month"
  | "last_month"
  | "last_30_days"
  | "this_year";

export const CATEGORIES = [
  "Food",
  "Groceries",
  "Transport",
  "Shopping",
  "Bills",
  "Entertainment",
  "Health",
  "Transfers",
  "Other",
] as const;

const SYMBOLS: Record<string, string> = {
  INR: "₹",
  USD: "$",
  EUR: "€",
  GBP: "£",
};

/** "₹1,299.50" — same as the backend's `expenses::money`. */
export function money(amountMinor: number, currency: string): string {
  const n = (amountMinor / 100).toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
  const sym = SYMBOLS[currency];
  return sym ? `${sym}${n}` : `${currency} ${n}`;
}
