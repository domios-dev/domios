// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx

import React, { act, useState } from "react";
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
  if (typeof (globalThis as { requestAnimationFrame?: unknown }).requestAnimationFrame !== "function") {
    (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame = (
      cb: FrameRequestCallback,
    ) => {
      cb(0);
      return 0;
    };
    (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
  }
}

async function openTabActionsMenu(tabRow: Element): Promise<void> {
  const trigger = tabRow.querySelector('button[title="Tab actions"]');
  assert(trigger?.parentElement, "tab actions trigger present");
  await act(async () => {
    trigger.parentElement!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  });
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
  assert(host.querySelector('[data-testid="role-badge-watcher"]'), "WATCHER shield on manager subline");
  assert(workerWrap?.textContent?.includes("Worker A"), "first worker rendered");
  assert(workerWrap?.textContent?.includes("Worker B"), "second worker rendered");
  assert(!workerWrap?.textContent?.includes("Watcher"), "Manager excluded from worker subtree");
  assert(host.textContent?.includes("Managers 1"), "Managers count is 1");
  assert(host.textContent?.includes("Workers 2"), "Workers count is 2");
  const managerIcon = managerWrap?.querySelector('[data-testid="tab-pane-icon"] .agent-icon');
  assert(managerIcon, "manager child row renders left provider icon");
  const workerRows = workerWrap?.querySelectorAll('[data-testid="tab-item"]');
  assert(workerRows && workerRows.length === 2, "each worker renders the shared tab row");
  const workerIcons = workerWrap?.querySelectorAll('[data-testid="tab-pane-icon"] .agent-icon');
  assert(workerIcons && workerIcons.length === 2, "each worker tab row renders left provider icon");
  const workerLevels = workerWrap?.querySelectorAll(".sidebar-hierarchy-level");
  assert(workerLevels && workerLevels.length > 0, "worker hierarchy levels present");
  for (const level of workerLevels) {
    assert(
      !level.className.includes("border-l"),
      "worker rows align flat with managers — no nested left stripe",
    );
  }
  await act(async () => root.unmount());
});

test("worker rows render in tab sort order when pane insertion order differs", async () => {
  const panes: Record<string, Pane> = {
    orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", agent_type: "claude" }),
    w2: pane({ id: "w2", title: "Worker B", role: "Worker", parent_pane_id: "orch", agent_type: "claude" }),
    w1: pane({ id: "w1", title: "Worker A", role: "Worker", parent_pane_id: "orch", agent_type: "claude" }),
  };
  const tabs = [
    tab("tab-orch", "orch", 0),
    tab("tab-w1", "w1", 1),
    tab("tab-w2", "w2", 2),
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

  const workerWrap = host.querySelector('[data-group-section-wrap="worker"]');
  const renderedIds = Array.from(workerWrap?.querySelectorAll("[data-tab-id]") ?? []).map((el) =>
    el.getAttribute("data-tab-id"),
  );
  assert(renderedIds.join(",") === "tab-w1,tab-w2", "worker DOM order follows tab sort_order");
  await act(async () => root.unmount());
});

test("clicking a worker tab row activates it with the shared active chrome", async () => {
  function Harness() {
    const [activeId, setActiveId] = useState("tab-orch");
    const panes: Record<string, Pane> = {
      orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", agent_type: "claude" }),
      w1: pane({ id: "w1", title: "Worker A", role: "Worker", parent_pane_id: "orch", agent_type: "claude" }),
    };
    const tabs = [tab("tab-orch", "orch", 0), tab("tab-w1", "w1", 1)];
    const workspace: Workspace = {
      id: "ws",
      name: "Workspace",
      tabs,
      active_tab_id: activeId,
      cwd: null,
    };
    return (
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={activeId}
          sortedTabs={tabs}
          panes={panes}
          paneStatuses={{}}
          planIndex={[]}
          plans={[]}
          activeWork={null}
          remoteWorkers={{}}
          closingTabIds={new Set()}
          appearingTabIds={new Set()}
          isActiveVisual={true}
          workspaceCanvasActive={false}
          touchMode={false}
          compact={false}
          sidebarActionText="text-xs"
          orchestrators={[panes.orch]}
          onBeforeOpenPlan={() => {}}
          onActivateTab={setActiveId}
          onCloseTab={() => {}}
          onRenameTab={() => {}}
          onNewSession={() => {}}
        />
      </SidebarDnd>
    );
  }

  const { host, root } = await render(<Harness />);
  const workerRow = host.querySelector('[data-tab-id="tab-w1"]') as HTMLElement;
  assert(workerRow, "worker tab row present");
  assert(workerRow.className.includes("sidebar-tab-dimmed"), "worker starts inactive");
  await act(async () => {
    workerRow.click();
  });
  const activeWorker = host.querySelector('[data-tab-id="tab-w1"]') as HTMLElement;
  assert(activeWorker, "worker tab row still present after click");
  assert(!activeWorker.className.includes("sidebar-tab-dimmed"), "active worker row drops dimmed class");
  await act(async () => root.unmount());
});

test("worker rows are single-line even with a description; plain tab single-line too", async () => {
  const panes: Record<string, Pane> = {
    orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", agent_type: "claude" }),
    w1: pane({
      id: "w1",
      title: "Worker A",
      role: "Worker",
      parent_pane_id: "orch",
      description: "fix sidebar rows",
      agent_type: "claude",
    }),
    plain: pane({ id: "plain", title: "Shell", role: null, agent_type: null }),
  };
  const tabs = [
    tab("tab-orch", "orch", 0),
    tab("tab-w1", "w1", 1),
    tab("tab-plain", "plain", 2),
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

  const workerSubline = host.querySelector('[data-tab-id="tab-w1"] [data-testid="tab-subline"]');
  assert(!workerSubline, "worker rows are single-line: no subline, no description");
  const plainSubline = host.querySelector('[data-tab-id="tab-plain"] [data-testid="tab-subline"]');
  assert(!plainSubline, "plain pane keeps a single line");
  await act(async () => root.unmount());
});

function tabIconScreenLeft(host: HTMLElement, tabId: string): number {
  const icon = host.querySelector(`[data-tab-id="${tabId}"] [data-testid="tab-pane-icon"]`) as HTMLElement;
  assert(icon, `icon for ${tabId}`);
  return icon.getBoundingClientRect().left;
}

test("childless and childful orchestrator heads keep the same icon x-position (overlay chevron, zero width)", async () => {
  const panes: Record<string, Pane> = {
    "orch-empty": pane({ id: "orch-empty", title: "Empty Orch", role: "Orchestrator", agent_type: "claude" }),
    "orch-full": pane({ id: "orch-full", title: "Full Orch", role: "Orchestrator", agent_type: "claude" }),
    worker: pane({ id: "worker", title: "Worker", role: "Worker", parent_pane_id: "orch-full", agent_type: "claude" }),
    plain: pane({ id: "plain", title: "Plain Tab", role: null, agent_type: "claude" }),
  };
  const tabs = [
    tab("tab-empty", "orch-empty", 0),
    tab("tab-full", "orch-full", 1),
    tab("tab-worker", "worker", 2),
    tab("tab-plain", "plain", 3),
  ];
  const workspace: Workspace = {
    id: "ws",
    name: "Workspace",
    tabs,
    active_tab_id: "tab-empty",
    cwd: null,
  };
  const { host, root } = await render(
    <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
      <SidebarWorkspaceTabs
        workspace={workspace}
        activeTabId="tab-empty"
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
        orchestrators={[panes["orch-empty"], panes["orch-full"]]}
        onBeforeOpenPlan={() => {}}
        onActivateTab={() => {}}
        onCloseTab={() => {}}
        onRenameTab={() => {}}
        onNewSession={() => {}}
      />
    </SidebarDnd>,
  );

  const emptyRow = host.querySelector('[data-tab-id="tab-empty"]')!;
  const fullRow = host.querySelector('[data-tab-id="tab-full"]')!;
  assert(!emptyRow.querySelector('[data-testid="group-collapse-toggle"]'), "childless head has no chevron");
  assert(fullRow.querySelector('[data-testid="group-collapse-toggle"]'), "childful head has overlay chevron");

  const emptyIconLeft = tabIconScreenLeft(host, "tab-empty");
  const fullIconLeft = tabIconScreenLeft(host, "tab-full");
  const plainIconLeft = tabIconScreenLeft(host, "tab-plain");
  assert(Math.abs(emptyIconLeft - fullIconLeft) < 0.5, "empty vs childful orchestrator icons share x-position");
  assert(Math.abs(emptyIconLeft - plainIconLeft) < 0.5, "orchestrator icon x matches a normal tab (no extra indent)");
  const inactiveTitle = host.querySelector('[data-tab-id="tab-empty"] [data-testid="tab-title"]') as HTMLElement;
  assert(inactiveTitle?.className.includes("text-[var(--ct-fg)]"), "inactive tab title uses full foreground");
  await act(async () => root.unmount());
});

test("worker rows expose close control and fire onCloseTab", async () => {
  const closedTabIds: string[] = [];
  const panes: Record<string, Pane> = {
    orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", agent_type: "claude" }),
    worker: pane({
      id: "worker",
      title: "Worker A",
      role: "Worker",
      parent_pane_id: "orch",
      agent_type: "claude",
    }),
  };
  const tabs = [tab("tab-orch", "orch", 0), tab("tab-worker", "worker", 1)];
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
        onCloseTab={(tabId) => {
          closedTabIds.push(tabId);
        }}
        onRenameTab={() => {}}
        onNewSession={() => {}}
      />
    </SidebarDnd>,
  );

  const row = host.querySelector('[data-tab-id="tab-worker"]');
  assert(row, "worker row rendered");
  await openTabActionsMenu(row);
  const closeItem = document.querySelector('[data-testid="tab-menu-close"]');
  assert(closeItem && !(closeItem as HTMLButtonElement).disabled, "close enabled for worker");
  await act(async () => {
    (closeItem as HTMLButtonElement).click();
  });
  assert(closedTabIds.includes("tab-worker"), "onCloseTab fired for worker");
  await act(async () => root.unmount());
});

test("manager rows expose close control and fire onCloseTab", async () => {
  const closedTabIds: string[] = [];
  const panes: Record<string, Pane> = {
    orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", agent_type: "claude" }),
    planner: pane({
      id: "planner",
      title: "Planner",
      role: "Manager",
      role_profile: "planner",
      parent_pane_id: "orch",
      agent_type: "claude",
    }),
    manager: pane({
      id: "manager",
      title: "Watcher",
      role: "Manager",
      role_profile: "watcher",
      parent_pane_id: "orch",
      agent_type: "codex",
    }),
  };
  const tabs = [
    tab("tab-orch", "orch", 0),
    tab("tab-planner", "planner", 1),
    tab("tab-manager", "manager", 2),
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
        onCloseTab={(tabId) => {
          closedTabIds.push(tabId);
        }}
        onRenameTab={() => {}}
        onNewSession={() => {}}
      />
    </SidebarDnd>,
  );

  for (const tabId of ["tab-planner", "tab-manager"] as const) {
    const row = host.querySelector(`[data-tab-id="${tabId}"]`);
    assert(row, `manager row rendered for ${tabId}`);
    await openTabActionsMenu(row);
    const closeItem = document.querySelector('[data-testid="tab-menu-close"]');
    assert(closeItem && !(closeItem as HTMLButtonElement).disabled, `close enabled for ${tabId}`);
    await act(async () => {
      (closeItem as HTMLButtonElement).click();
    });
    assert(closedTabIds.includes(tabId), `onCloseTab fired for ${tabId}`);
    document.body.querySelectorAll("[data-testid='tab-menu-close']").forEach((el) => el.remove());
  }

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
