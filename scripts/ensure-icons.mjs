// Generates the Tauri icon set from assets/igris-logo.svg if it is missing.
// Tauri cannot compile without icons, and binary icons may not be committed.
import { execSync } from "node:child_process";
import { existsSync } from "node:fs";

if (!existsSync("src-tauri/icons/icon.png")) {
  console.log("[igris] Generating app icons from assets/igris-logo.svg");
  execSync("npx tauri icon assets/igris-logo.svg -o src-tauri/icons", { stdio: "inherit" });
}
