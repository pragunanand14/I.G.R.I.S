import { RoutinesList } from "@/components/routines/RoutinesList";
import { PageHeader } from "@/components/ui/PageHeader";

/** Things IGRIS does by itself. */
export function RoutinesPage() {
  return (
    <div className="mx-auto max-w-3xl px-8 pt-8 pb-16">
      <PageHeader title="Routines" description="Things IGRIS does by itself — on a schedule, or when an email arrives." />
      <RoutinesList />
    </div>
  );
}
