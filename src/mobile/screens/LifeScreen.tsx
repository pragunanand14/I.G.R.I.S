import { useState } from "react";
import { PeopleList } from "@/components/life/PeopleList";
import { Spending } from "@/components/life/Spending";
import { useAppStore } from "@/stores/appStore";
import { Screen } from "../ui";

/** The people in your life and what you spend. */
export function LifeScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  const [tab, setTab] = useState<"people" | "spending">("people");
  return (
    <Screen
      title="Life"
      subtitle="The people in your life and what you spend."
      back="/settings"
    >
      {ready ? (
        <div className="space-y-3">
          <div className="flex gap-2" role="group" aria-label="Show">
            <button
              type="button"
              aria-pressed={tab === "people"}
              className={`m-chip ${tab === "people" ? "m-accent" : ""}`}
              onClick={() => setTab("people")}
            >
              People
            </button>
            <button
              type="button"
              aria-pressed={tab === "spending"}
              className={`m-chip ${tab === "spending" ? "m-accent" : ""}`}
              onClick={() => setTab("spending")}
            >
              Spending
            </button>
          </div>
          {tab === "people" ? <PeopleList phone /> : <Spending phone />}
        </div>
      ) : (
        <p className="m-muted">This appears once IGRIS is running.</p>
      )}
    </Screen>
  );
}
