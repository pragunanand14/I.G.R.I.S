import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "@/services/api";
import { useAlertStore } from "@/stores/alertStore";
import type { FiredReminder } from "@/types/productivity";
import { ReminderAlerts } from "./ReminderAlerts";

const fired = (over: Partial<FiredReminder>): FiredReminder => ({
  id: 1,
  title: "Stretch",
  kind: "reminder",
  dueAt: "2026-10-05T10:00:00Z",
  durationSecs: null,
  status: "fired",
  firedAt: "2026-10-05T10:00:00Z",
  createdAt: "2026-10-05T09:00:00Z",
  late: false,
  ...over,
});

describe("ReminderAlerts", () => {
  beforeEach(() => useAlertStore.setState({ ringing: [], revision: 0 }));

  it("renders nothing when nothing is ringing", () => {
    const { container } = render(<ReminderAlerts enabled={false} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows ringing reminders, timers and missed ones", () => {
    useAlertStore.setState({ ringing: [fired({}), fired({ id: 2, kind: "timer", title: "Tea" }), fired({ id: 3, title: "Old", late: true })] });
    render(<ReminderAlerts enabled={false} />);
    expect(screen.getAllByRole("alert")).toHaveLength(3);
    expect(screen.getByText("Timer finished")).toBeInTheDocument();
    expect(screen.getByText("Missed reminder")).toBeInTheDocument();
    expect(screen.getByText(/while IGRIS was closed/)).toBeInTheDocument();
  });

  it("snoozes and dismisses through the backend", async () => {
    const snooze = vi.spyOn(api, "snoozeReminder").mockResolvedValue({ ...fired({}), status: "pending" });
    const dismiss = vi.spyOn(api, "dismissReminder").mockResolvedValue({ ...fired({ id: 2 }), status: "dismissed" });
    useAlertStore.setState({ ringing: [fired({}), fired({ id: 2, title: "Tea" })] });
    render(<ReminderAlerts enabled={false} />);
    fireEvent.click(screen.getAllByText("Snooze 10 min")[0]!);
    await waitFor(() => expect(snooze).toHaveBeenCalledWith(1, 10));
    fireEvent.click(screen.getByText("Done"));
    await waitFor(() => expect(dismiss).toHaveBeenCalledWith(2));
    await waitFor(() => expect(useAlertStore.getState().ringing).toHaveLength(0));
  });
});
