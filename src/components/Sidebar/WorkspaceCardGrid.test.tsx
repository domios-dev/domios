import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../lib/types";
import { SidebarDnd } from "./sortable";
import { WorkspaceCardGrid, workspaceCardPosition } from "./WorkspaceCardGrid";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  HTMLInputElement: window.HTMLInputElement,
  Element: window.Element,
  Node: window.Node,
  Event: window.Event,
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

async function run() {
  assert.deepEqual(
    Array.from({ length: 8 }, (_, index) => workspaceCardPosition(index, 3)),
    [
      { gridColumn: 1, gridRow: 1 },
      { gridColumn: 2, gridRow: 1 },
      { gridColumn: 3, gridRow: 1 },
      { gridColumn: 1, gridRow: 2 },
      { gridColumn: 2, gridRow: 2 },
      { gridColumn: 3, gridRow: 2 },
      { gridColumn: 4, gridRow: 1 },
      { gridColumn: 5, gridRow: 1 },
    ],
    "cards fill left-to-right before using the second row and horizontal overflow",
  );
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

  assert.equal(host.querySelector('[role="region"][aria-label="Workspace cards"]')?.querySelectorAll('[role="listitem"]').length, 2);
  assert.equal(host.querySelector('[aria-label="Select workspace CodeTerm"]')?.getAttribute("aria-current"), "true");
  assert(host.querySelector('[role="status"][aria-label="CodeTerm has working tabs"]'), "working workspace exposes accessible activity");
  assert(host.textContent?.includes("CT") && host.textContent.includes("TS"), "derived initials render");

  await act(async () => (host.querySelector('[aria-label="Select workspace team_smoke"]') as HTMLButtonElement).click());
  assert.deepEqual(selected, ["two"]);
  await act(async () => (host.querySelector('[aria-label="Edit initials for CodeTerm"]') as HTMLButtonElement).click());
  const input = host.querySelector('[aria-label="Initials for CodeTerm"]') as HTMLInputElement;
  assert.equal(input.maxLength, 2, "initials editor enforces the two-character contract");

  await act(async () => root.unmount());
  console.log("workspace card grid tests: passed");
}

void run();
