import { SmartHome } from "@/components/home/SmartHome";
import { PageHeader } from "@/components/ui/PageHeader";

export function SmartHomePage() {
  return (
    <div className="mx-auto max-w-3xl px-8 pt-8 pb-16">
      <PageHeader title="Smart home" description="Control your TV from here, or just ask IGRIS — “turn the volume down”, “open Netflix on the TV”." />
      <SmartHome />
    </div>
  );
}
