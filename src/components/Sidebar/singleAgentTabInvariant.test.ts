import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const shippedSources = [
  "src/components/Sidebar/Sidebar.tsx",
  "src/components/Sidebar/SidebarPlanPeerPair.tsx",
  "src/components/Sidebar/SidebarWorkspaceTabs.tsx",
  "src/components/Sidebar/TabItem.tsx",
  "src/components/Sidebar/tabStatuses.ts",
  "src/components/Sidebar/workspaceTabs.ts",
  "src-tauri/crates/codeterm-pty/src/multiplexer/state/tab.rs",
];
const removedSymbols = [
  "collect" + "TabIds",
  "tab" + "Count",
  "let tab_ids = " + "vec![tab.tab_id",
];

for (const relativePath of shippedSources) {
  const source = readFileSync(resolve(root, relativePath), "utf8");
  for (const symbol of removedSymbols) {
    assert.equal(
      source.includes(symbol),
      false,
      `${relativePath} still contains obsolete multi-agent Tab symbol: ${symbol}`,
    );
  }
}

const rustTab = readFileSync(
  resolve(root, "src-tauri/crates/codeterm-pty/src/multiplexer/types.rs"),
  "utf8",
);
const rustTabBody = rustTab.match(/pub struct Tab \{([\s\S]*?)\n\}/)?.[1] ?? "";
assert.match(rustTabBody, /pub id: String,/);
assert.doesNotMatch(rustTabBody, /pub (?:tab_id|tab_ids|agent_ids):/);

const tsTab = readFileSync(resolve(root, "src/lib/types.ts"), "utf8");
const tsTabBody = tsTab.match(/export interface Tab \{([\s\S]*?)\n\}/)?.[1] ?? "";
assert.match(tsTabBody, /\bid: string;/);
assert.doesNotMatch(tsTabBody, /\b(?:tab_id|tab_ids|agent_ids):/);

console.log("single-agent Tab invariant: ok");
