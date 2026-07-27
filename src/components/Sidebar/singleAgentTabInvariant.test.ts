import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const shippedSources = [
  "src/components/Sidebar/Sidebar.tsx",
  "src/components/Sidebar/SidebarPlanPeerPair.tsx",
  "src/components/Sidebar/SidebarWorkspaceTabs.tsx",
  "src/components/Sidebar/TabItem.tsx",
  "src/components/Sidebar/paneStatuses.ts",
  "src/components/Sidebar/workspaceTabs.ts",
  "src-tauri/crates/codeterm-pty/src/multiplexer/state/tab.rs",
];
const removedSymbols = [
  "collect" + "PaneIds",
  "aggregate" + "TabStatus",
  "pane" + "Count",
  "let pane_ids = " + "vec![tab.pane_id",
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
assert.match(rustTab, /pub struct Tab \{[\s\S]*?pub pane_id: String,/);
assert.doesNotMatch(rustTab, /pub struct Tab \{[\s\S]*?pub (?:pane|agent)_ids:/);

const tsTab = readFileSync(resolve(root, "src/lib/types.ts"), "utf8");
assert.match(tsTab, /export interface Tab \{[\s\S]*?pane_id: string;/);
assert.doesNotMatch(tsTab, /export interface Tab \{[\s\S]*?(?:pane|agent)_ids:/);

console.log("single-agent Tab invariant: ok");
