import { RoutinesList } from "@/components/routines/RoutinesList";
import { useAppStore } from "@/stores/appStore";
import { Screen } from "../ui";

/** Things IGRIS does by itself. */
export function RoutinesScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  return (
    <Screen
      title="Routines"
      subtitle="Things IGRIS does by itself — on a schedule, or when an email arrives."
      back="/settings"
    >
      {ready ? (
        <RoutinesList phone />
      ) : (
        <p className="m-muted">This appears once IGRIS is running.</p>
      )}
    </Screen>
  );
}
