// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx

import React, { act } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import type { Pane, Tab, Workspace } from "../../lib/types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function pane(overrides: Partial<Pane>): Pane {
  return {
    id: "p",
    pty_id: null,
    pane_type: "Terminal",
    title: "p",
    cwd: null,
    role: null,
    role_profile: null,
    parent_pane_id: null,
    ...overrides,
  } as Pane;
}

function tab(id: string, paneId: string, sortOrder: number): Tab {
  return { id, pane_id: paneId, sort_order: sortOrder };
}

setupDom();

test("manager child without plan binding renders in Managers and workers remain in Workers", async () => {
  const panes: Record<string, Pane> = {
    orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", agent_type: "claude" }),
    manager: pane({
      id: "manager",
      title: "Watcher",
      role: "Manager",
      role_profile: "watcher",
      parent_pane_id: "orch",
      agent_type: "codex",
    }),
    w1: pane({ id: "w1", title: "Worker A", role: "Worker", parent_pane_id: "orch", agent_type: "claude" }),
    w2: pane({ id: "w2", title: "Worker B", role: "Worker", parent_pane_id: "orch", agent_type: "opencode" }),
  };
  const tabs = [
    tab("tab-orch", "orch", 0),
    tab("tab-manager", "manager", 1),
    tab("tab-w1", "w1", 2),
    tab("tab-w2", "w2", 3),
  ];
  const workspace: Workspace = {
    id: "ws",
    name: "Workspace",
    tabs,
    active_tab_id: "tab-orch",
    cwd: null,
  };
  const { host, root } = await render(
    <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
      <SidebarWorkspaceTabs
        workspace={workspace}
        activeTabId="tab-orch"
        sortedTabs={tabs}
        panes={panes}
        paneStatuses={{}}
        planIndex={[]}
        plans={[]}
        activeWork={null}
        remoteWorkers={{}}
        closingTabIds={new Set()}
        appearingTabIds={new Set()}
        isActiveVisual={false}
        workspaceCanvasActive={false}
        touchMode={false}
        compact={false}
        sidebarActionText="text-xs"
        orchestrators={[panes.orch]}
        onBeforeOpenPlan={() => {}}
        onActivateTab={() => {}}
        onCloseTab={() => {}}
        onRenameTab={() => {}}
        onNewSession={() => {}}
      />
    </SidebarDnd>,
  );

  assert(host.querySelector('[data-testid="group-section-managers"]'), "Managers header present");
  assert(host.querySelector('[data-testid="group-section-workers"]'), "Workers header present");
  const managerWrap = host.querySelector('[data-group-section-wrap="manager"]');
  const workerWrap = host.querySelector('[data-group-section-wrap="worker"]');
  assert(managerWrap?.textContent?.includes("Watcher"), "Manager row rendered without plan binding");
  assert(host.querySelector('[data-testid="role-badge-watcher"]'), "WATCHER badge on manager row");
  assert(workerWrap?.textContent?.includes("Worker A"), "first worker rendered");
  assert(workerWrap?.textContent?.includes("Worker B"), "second worker rendered");
  assert(!workerWrap?.textContent?.includes("Watcher"), "Manager excluded from worker subtree");
  assert(host.textContent?.includes("Managers 1"), "Managers count is 1");
  assert(host.textContent?.includes("Workers 2"), "Workers count is 2");
  const managerIcon = managerWrap?.querySelector('[data-testid="tab-pane-icon"] .agent-icon');
  assert(managerIcon, "manager child row renders left provider icon");
  const workerIcons = workerWrap?.querySelectorAll('[data-testid="hierarchy-pane-icon"] .agent-icon');
  assert(workerIcons && workerIcons.length === 2, "each worker hierarchy row renders left provider icon");
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`PASS ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`SidebarWorkspaceTabs tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
