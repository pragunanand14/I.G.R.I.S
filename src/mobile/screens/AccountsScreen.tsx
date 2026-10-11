import { GoogleAccount } from "@/components/accounts/GoogleAccount";
import { useAppStore } from "@/stores/appStore";
import { Screen } from "../ui";

/** Settings → Accounts: Gmail and Google Calendar. */
export function AccountsScreen() {
  const ready = useAppStore((s) => s.backend) === "ready";
  return (
    <Screen title="Accounts" back="/settings">
      <div className="m-card px-4 py-4">{ready ? <GoogleAccount phone /> : <p className="m-muted">This appears once IGRIS is running.</p>}</div>
    </Screen>
  );
}
