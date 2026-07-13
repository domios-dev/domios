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
assert(
  css.includes(".new-session-picker-card .pair-affordance--card {\n  display: grid;"),
  "picker pair affordance uses a grid layout",
);
assert(css.includes("grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);"), "pair layout can shrink within the card");
assert(css.includes("@media (max-width: 640px)"), "pair layout has a narrow viewport rule");

console.log("NewSessionPicker layout tests passed");
