import { useAppStore } from "@/stores/appStore";
import { useAssistantStore } from "@/stores/assistantStore";
import type { CoreState } from "@/types/assistant";

/** The core shows ERROR whenever the native backend is not healthy. */
export function useCoreState(): CoreState {
  const backend = useAppStore((s) => s.backend);
  const activity = useAssistantStore((s) => s.activity);
  if (backend === "unavailable" || backend === "error") return "error";
  return activity;
}
