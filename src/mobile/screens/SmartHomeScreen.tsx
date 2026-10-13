import { SmartHome } from "@/components/home/SmartHome";
import { Screen } from "../ui";

/** Smart home: the TV remote, and adding devices. */
export function SmartHomeScreen() {
  return (
    <Screen
      title="Smart home"
      subtitle="Your TV, from your phone. Or just ask IGRIS."
      back="/"
    >
      <SmartHome phone />
    </Screen>
  );
}
