// Run: npx tsx src/components/Terminal/NewSessionPicker.layout.test.ts

import { readFileSync } from "fs";
import { join } from "path";
import { fileURLToPath } from "url";

const here = fileURLToPath(new URL(".", import.meta.url));
const picker = readFileSync(join(here, "NewSessionPicker.tsx"), "utf8");
const css = readFileSync(join(here, "../../styles/globals.css"), "utf8");

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(!picker.includes("handleQuickProviderStart"), "picker does not keep the legacy quick provider list");
assert(!picker.includes("New {provider.display_name} Session"), "picker does not render provider session rows below mode cards");
assert(picker.includes("fixed inset-0"), "picker is a modal overlay instead of tab content");
assert(picker.includes('aria-modal="true"'), "picker exposes modal accessibility semantics");
assert(picker.includes('aria-label="Close new session dialog"'), "picker has an accessible close action");
assert(picker.includes("backdrop-blur-sm"), "picker separates the modal from the current tab with backdrop blur");
assert(!picker.includes("closeSessionPicker"), "picker no longer depends on synthetic tab cleanup");
assert(
  /\.new-session-picker-card \.pair-affordance--card\s*\{[^}]*display:\s*grid;/s.test(css),
  "picker pair affordance uses a grid layout",
);
assert(css.includes("grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);"), "pair layout can shrink within the card");
assert(css.includes("@media (max-width: 640px)"), "pair layout has a narrow viewport rule");

console.log("NewSessionPicker layout tests passed");
