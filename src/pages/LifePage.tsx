import { useState } from "react";
import { PeopleList } from "@/components/life/PeopleList";
import { Spending } from "@/components/life/Spending";
import { PageHeader } from "@/components/ui/PageHeader";
import { Segmented } from "@/components/ui/Segmented";

type Tab = "people" | "spending";

/** What IGRIS knows about your life: the people in it, and your spending. */
export function LifePage() {
  const [tab, setTab] = useState<Tab>("people");
  return (
    <div className="mx-auto max-w-3xl px-8 pt-8 pb-16">
      <PageHeader
        title="Life"
        description="The people in your life and what you spend — so IGRIS can help without asking every time."
      />
      <div className="mb-4">
        <Segmented<Tab>
          label="Show"
          value={tab}
          onChange={setTab}
          options={[
            { value: "people", label: "People" },
            { value: "spending", label: "Spending" },
          ]}
        />
      </div>
      {tab === "people" ? <PeopleList /> : <Spending />}
    </div>
  );
}
