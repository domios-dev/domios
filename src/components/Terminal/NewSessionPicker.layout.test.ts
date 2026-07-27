// Run: npx tsx src/components/Terminal/NewSessionPicker.layout.test.ts

import { readFileSync } from "fs";
import { join } from "path";
import { fileURLToPath } from "url";

const here = fileURLToPath(new URL(".", import.meta.url));
const picker = readFileSync(join(here, "NewSessionPicker.tsx"), "utf8");
const pickerParts = readFileSync(join(here, "NewSessionPickerParts.tsx"), "utf8");
const css = readFileSync(join(here, "../../styles/globals.css"), "utf8");

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(!picker.includes("handleQuickProviderStart"), "picker does not keep the legacy quick provider list");
assert(!picker.includes("New {provider.display_name} Session"), "picker does not render provider session rows below mode cards");
assert(pickerParts.includes('label="Team"'), "pair mode uses the concise Team label");
assert(picker.includes('"Start team"'), "pair mode action uses the Team label");
assert(picker.includes("fixed inset-0"), "picker is a modal overlay instead of tab content");
assert(picker.includes("return createPortal("), "picker escapes sidebar and Tab stacking contexts through a body portal");
assert(picker.includes("document.body"), "picker portal covers the entire application viewport");
assert(picker.includes('aria-modal="true"'), "picker exposes modal accessibility semantics");
assert(picker.includes('aria-label="Close new session dialog"'), "picker has an accessible close action");
assert(picker.includes("backdrop-blur-sm"), "picker separates the modal from the current tab with backdrop blur");
assert(picker.includes("new-session-picker-card picker-stagger relative my-auto"), "popup card participates only in the outer overlay scroll");
assert(picker.includes("overflow-hidden rounded-xl"), "popup card never creates an inner scrollbar");
assert(!picker.includes("max-h-[calc(100vh-3rem)]"), "popup does not use an internally scrolling viewport");
assert(/\.new-session-picker\s*\{[^}]*position:\s*fixed;[^}]*inset:\s*0;[^}]*z-index:\s*1000;/s.test(css), "popup CSS cannot override the full-screen fixed portal");
assert(!picker.includes("closeSessionPicker"), "picker no longer depends on synthetic tab cleanup");
assert(
  /\.new-session-picker-card \.pair-affordance--card\s*\{[^}]*display:\s*grid;/s.test(css),
  "picker pair affordance uses a grid layout",
);
assert(css.includes("grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);"), "pair layout can shrink within the card");
assert(css.includes("@media (max-width: 640px)"), "pair layout has a narrow viewport rule");

console.log("NewSessionPicker layout tests passed");
