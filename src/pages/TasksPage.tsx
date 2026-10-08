import { useCallback, useState } from "react";
import { CalendarPanel } from "@/components/productivity/CalendarPanel";
import { RemindersPanel } from "@/components/productivity/RemindersPanel";
import { TaskList } from "@/components/productivity/TaskList";
import { PageHeader } from "@/components/ui/PageHeader";
import { Segmented } from "@/components/ui/Segmented";
import { useAppStore } from "@/stores/appStore";

type Tab = "tasks" | "reminders" | "calendar";

const TAB_KEY = "igris.tasks.tab";

function initialTab(): Tab {
  try {
    const t = localStorage.getItem(TAB_KEY);
    return t === "reminders" || t === "calendar" ? t : "tasks";
  } catch {
    return "tasks";
  }
}

export function TasksPage() {
  const backend = useAppStore((s) => s.backend);
  const [tab, setTab] = useState<Tab>(initialTab);
  const [error, setError] = useState<string | null>(null);
  const onError = useCallback((m: string | null) => setError(m), []);

  const choose = (t: Tab) => {
    setTab(t);
    setError(null);
    try {
      localStorage.setItem(TAB_KEY, t);
    } catch {
      // Remembering the tab is optional.
    }
  };

  if (backend !== "ready") {
    return (
      <div className="mx-auto max-w-3xl px-8 pt-8">
        <PageHeader title="Tasks" />
        <p className="text-sm text-danger">Tasks and reminders require the IGRIS desktop backend.</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl px-8 pt-8 pb-16">
      <PageHeader
        title="Tasks"
        description="Your to-dos, reminders, timers and calendar. IGRIS can manage all of them from chat."
        action={
          <Segmented<Tab>
            label="Section"
            value={tab}
            onChange={choose}
            options={[
              { value: "tasks", label: "Tasks" },
              { value: "reminders", label: "Reminders" },
              { value: "calendar", label: "Calendar" },
            ]}
          />
        }
      />
      {error && (
        <p role="alert" className="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}
      {tab === "tasks" && <TaskList onError={onError} />}
      {tab === "reminders" && <RemindersPanel onError={onError} />}
      {tab === "calendar" && <CalendarPanel onError={onError} />}
    </div>
  );
}
