import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Workspace } from "../../lib/types";
import { SidebarDnd } from "./sortable";
import { mergeVisibleOrder, visibleWorkspaceIds, WorkspaceCardGrid } from "./WorkspaceCardGrid";

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
  tabs: Array.from({ length: tabs }, (_, index) => ({ id: `tab-${index}`, tab_id: `tab-${index}`, title: `Tab ${index}` } as never)),
  active_tab_id: null,
  cwd: null,
});
const workspaces = [workspace("one", "CodeTerm", 2), workspace("two", "team_smoke")];
const selected: string[] = [];
let creates = 0;
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

async function run() {
  const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  assert.match(
    styles,
    /\.sidebar-workspace-card-working\s*\{[^}]*width:\s*12px;[^}]*height:\s*7px;[^}]*box-sizing:\s*border-box;[^}]*padding-bottom:\s*0\.75px;[^}]*align-items:\s*center;[^}]*border-radius:\s*999px;[^}]*background:\s*color-mix\(in srgb, var\(--ct-accent\) 62%, var\(--ct-surface\)\);[^}]*color:\s*var\(--ct-on-accent\);[^}]*box-shadow:\s*0 0 0 1px color-mix\(in srgb, var\(--ct-accent\) 22%, transparent\);/s,
    "working indicator is a compact theme-token pill with an optically centered dot baseline",
  );
  const ids = ["a", "b", "c", "d", "e", "f", "g", "h"];
  assert.deepEqual(visibleWorkspaceIds(ids.slice(0, 5), null, 3, false), { visible: ["a", "b", "c", "d", "e"], hidden: 0 }, "everything plus create fits in two rows");
  assert.deepEqual(visibleWorkspaceIds(ids, null, 3, false), { visible: ["a", "b", "c", "d", "e"], hidden: 3 }, "two rows: five cards and a +N cell, no horizontal overflow");
  assert.deepEqual(visibleWorkspaceIds(ids, "h", 3, false).visible, ["a", "b", "c", "d", "h"], "the selected workspace stays visible when collapsed");
  assert.deepEqual(visibleWorkspaceIds(ids, null, 3, true), { visible: ids, hidden: 0 }, "expanded shows every workspace");
  assert.deepEqual(mergeVisibleOrder(ids, ["b", "a", "c", "d", "h"]), ["b", "a", "c", "d", "e", "f", "g", "h"], "a reorder of the visible cards keeps hidden ones in place");
  await act(async () => root.render(
    <SidebarDnd handlers={{ reorderTabs: () => {}, reorderDisplay: () => {}, reorderWorkspaces: () => {} }}>
      <WorkspaceCardGrid
        workspaces={workspaces}
        selectedWorkspaceId="one"
        statuses={{ one: "Working", two: "Working" }}
        sortIds={["ws:one", "ws:two"]}
        onSelect={(id) => selected.push(id)}
        onCreate={() => { creates += 1; }}
      />
    </SidebarDnd>,
  ));

  assert.equal(host.querySelector('[role="region"][aria-label="Workspace cards"]')?.querySelectorAll('[role="listitem"]').length, 2);
  assert.equal(host.querySelector('[data-testid="workspace-card-more"]'), null, "no +N control when everything fits");
  assert.equal(host.querySelector('[aria-label="Select workspace CodeTerm"]')?.getAttribute("aria-current"), "true");
  const secondGridItem = host.querySelector('[data-workspace-id="two"]')?.parentElement as HTMLElement;
  const createButton = host.querySelector('[aria-label="Create workspace"]') as HTMLButtonElement;
  assert.equal(secondGridItem.nextElementSibling, createButton, "create sits in the next free cell right after the last card");
  await act(async () => createButton.click());
  assert.equal(creates, 1, "create card owns the workspace creation action");
  assert.equal(
    host.querySelector('[role="status"][aria-label="CodeTerm has working tabs"]'),
    null,
    "selected workspace does not duplicate its active state with an activity badge",
  );
  const workingIndicator = host.querySelector('[role="status"][aria-label="team_smoke has working tabs"]');
  assert(workingIndicator, "unselected working workspace exposes accessible activity");
  const workingDots = workingIndicator.querySelector('.activity-dots');
  assert(workingDots?.classList.contains("activity-dots--micro"), "working workspace uses micro activity dots");
  assert(workingDots?.classList.contains("activity-dots--workspace-badge"), "working workspace uses the badge-only two-dot variant");
  assert(
    workingDots?.classList.contains("activity-dots--on-accent"),
    "working workspace uses the filled-surface contrast tone",
  );
  assert.equal(
    workingIndicator.querySelector('[aria-hidden="true"]')?.children.length,
    2,
    "working workspace uses exactly two compact activity dots",
  );
  assert(host.textContent?.includes("CT") && host.textContent.includes("TS"), "derived initials render");

  await act(async () => (host.querySelector('[aria-label="Select workspace team_smoke"]') as HTMLButtonElement).click());
  assert.deepEqual(selected, ["two"]);
  await act(async () => (host.querySelector('[aria-label="Edit initials for CodeTerm"]') as HTMLButtonElement).click());
  const dialog = document.querySelector('[role="dialog"][aria-label="Edit CodeTerm workspace card"]');
  assert(dialog, "editor renders through a portal beside the card");
  const input = dialog.querySelector('[aria-label="Initials for CodeTerm"]') as HTMLInputElement;
  assert.equal(input.maxLength, 2, "initials editor enforces the two-character contract");
  assert(!dialog.querySelector('[role="radiogroup"]'), "editor has no per-workspace color controls");
  await act(async () => (dialog.querySelector(".sidebar-workspace-card-popover-done") as HTMLButtonElement).click());
  assert(!document.querySelector('[role="dialog"][aria-label="Edit CodeTerm workspace card"]'), "Done closes the portal editor");

  await act(async () => root.unmount());
  console.log("workspace card grid tests: passed");
}

void run();
