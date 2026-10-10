import { AllowedActions } from "@/components/security/AllowedActions";
import { useAppStore } from "@/stores/appStore";
import { Screen } from "../ui";

/** Actions IGRIS may do without asking, because you chose "Always allow". */
export function PermissionsScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  return (
    <Screen title="Always allowed" subtitle="IGRIS does these without asking. Remove one and it asks again next time." back="/settings">
      {ready ? <AllowedActions phone /> : <p className="m-muted">This appears once IGRIS is running.</p>}
    </Screen>
  );
}
