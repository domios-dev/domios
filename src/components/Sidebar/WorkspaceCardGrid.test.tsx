import assert from "node:assert/strict";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../lib/types";
import { SidebarDnd } from "./sortable";
import { WorkspaceCardGrid } from "./WorkspaceCardGrid";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
  Node: window.Node,
  localStorage: window.localStorage,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const workspace = (id: string, name: string, tabs = 0): Workspace => ({
  id,
  name,
  tabs: Array.from({ length: tabs }, (_, index) => ({ id: `tab-${index}`, pane_id: `pane-${index}`, title: `Tab ${index}` } as never)),
  active_tab_id: null,
  cwd: null,
});
const workspaces = [workspace("one", "CodeTerm", 2), workspace("two", "team_smoke")];
const selected: string[] = [];
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

await act(async () => root.render(
  <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
    <WorkspaceCardGrid
      workspaces={workspaces}
      selectedWorkspaceId="one"
      statuses={{ one: "Working", two: null }}
      sortIds={["ws:one", "ws:two"]}
      onSelect={(id) => selected.push(id)}
    />
  </SidebarDnd>,
));

assert.equal(host.querySelectorAll('[data-testid="workspace-card"]').length, 2);
assert.equal(host.querySelector('[data-testid="workspace-card"][data-workspace-id="one"]')?.getAttribute("data-selected"), "true");
assert(host.querySelector('[data-workspace-id="one"] .sidebar-workspace-card-working'), "working workspace exposes activity");
assert(host.textContent?.includes("CT") && host.textContent.includes("TS"), "derived initials render");

await act(async () => (host.querySelector('[aria-label="Select workspace team_smoke"]') as HTMLButtonElement).click());
assert.deepEqual(selected, ["two"]);
await act(async () => (host.querySelector('[aria-label="Edit initials for CodeTerm"]') as HTMLButtonElement).click());
const input = host.querySelector('[aria-label="Initials for CodeTerm"]') as HTMLInputElement;
await act(async () => {
  input.value = "zz";
  input.dispatchEvent(new window.Event("input", { bubbles: true }));
  input.blur();
});
assert(host.textContent?.includes("ZZ"), "custom initials replace derived initials");

await act(async () => root.unmount());
console.log("workspace card grid tests: passed");
