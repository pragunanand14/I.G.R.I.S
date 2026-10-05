import { useAppStore } from "@/stores/appStore";
import { useAssistantStore } from "@/stores/assistantStore";
import { phaseToCore, useOperatorStore } from "@/stores/operatorStore";
import type { CoreState } from "@/types/assistant";

/**
 * The core shows ERROR whenever the native backend is not healthy. While IGRIS
 * operates the computer it shows the operator phase (planning, executing,
 * verifying…), except when voice is listening or speaking.
 */
export function useCoreState(): CoreState {
  const backend = useAppStore((s) => s.backend);
  const activity = useAssistantStore((s) => s.activity);
  const phase = useOperatorStore((s) => (s.snapshot.active ? s.snapshot.task?.phase : undefined));
  if (backend === "unavailable" || backend === "error") return "error";
  if (phase && activity !== "listening" && activity !== "speaking") return phaseToCore(phase);
  return activity;
}
