// Shared harness for the SidebarWorkspaceTabs suites. Not a test file.

import { readFileSync } from "node:fs";
import { Window } from "happy-dom";
import type { Root } from "react-dom/client";
import type { Tab, ProviderDescriptor, Workspace } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import type { TeamMemberRole } from "../../lib/teamActions";
import { __setListenForTest } from "../../lib/bridge";
import { TEAM } from "./SidebarWorkspaceTabs.fixtures.testdata";

export let act: typeof import("react").act;
export let useState: typeof import("react").useState;
export let createRoot: typeof import("react-dom/client").createRoot;

type TestFn = () => void | Promise<void>;

/// One registry per spec file. `run` loads React, executes every registered case,
/// and exits non-zero on the first failure — so a suite that registers nothing
/// reports 0/0 rather than passing silently.
export function createSuite(label: string) {
  const tests: Array<[string, TestFn]> = [];
  const test = (name: string, fn: TestFn) => {
    tests.push([name, fn]);
  };
  const run = async () => {
    __setListenForTest(async () => () => {});
    ({ act, useState } = await import("react"));
    ({ createRoot } = await import("react-dom/client"));

    let failed = 0;
    for (const [name, fn] of tests) {
      try {
        window.localStorage.clear();
        await fn();
        console.log(`✓ ${name}`);
      } catch (error) {
        failed += 1;
        console.error(`✗ ${name}`);
        console.error(error);
      }
    }
    if (failed > 0) process.exit(1);
    __setListenForTest(null);
    if (tests.length === 0) {
      console.error(`✗ ${label}: registered no tests`);
      process.exit(1);
    }
    console.log(`
✓ ${tests.length}/${tests.length} ${label} tests passed`);
  };
  return { test, run };
}

export function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

export function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
  (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (callback) => {
      callback(0);
      return 0;
    };
  (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
}

export const sidebarStyles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");

export async function openTabActionsMenu(tabRow: Element): Promise<void> {
  const trigger = tabRow.querySelector('button[title="Tab actions"]');
  assert(trigger?.parentElement, "tab actions trigger present");
  await act(async () => {
    trigger.parentElement!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  });
}

export async function renderSidebar({
  tabs,
  sortedTabs = Object.values(tabs).map((item) => item.id),
  plans = [],
  planIndex = [],
  statuses = {},
  providers = [],
  activeTabId,
  onActivateTab = () => {},
  onCloseTab = () => {},
  onRenameTab = () => {},
  onRenameTeam,
  onAddTeamMember,
  onCloseTeam = () => {},
  newSessionDisabled = false,
}: {
  tabs: Record<string, Tab>;
  sortedTabs?: string[];
  plans?: Plan[];
  planIndex?: PlanIndexEntry[];
  statuses?: Record<string, "Working" | "Waiting" | "Idle" | "Dead">;
  providers?: ProviderDescriptor[];
  activeTabId?: string | null;
  onActivateTab?: (tabId: string) => void;
  onCloseTab?: (tabId: string) => void;
  onRenameTab?: (tabId: string, title: string) => void;
  onRenameTeam?: (teamId: string, name: string, managerTabId: string | null) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string | null,
    model?: string,
  ) => void;
  onCloseTeam?: (teamId: string) => void;
  newSessionDisabled?: boolean;
}): Promise<{
  host: HTMLDivElement;
  root: Root;
  rerender: (nextPlans: Plan[], nextPlanIndex?: PlanIndexEntry[]) => Promise<void>;
}> {
  const workspace: Workspace = {
    id: "workspace",
    name: "Workspace",
    tabs: sortedTabs,
    active_tab_id: activeTabId ?? sortedTabs[0] ?? null,
    cwd: "/workspace",
  };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const rerender = async (nextPlans: Plan[], nextPlanIndex = planIndex) => act(async () => {
    root.render(
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={workspace.active_tab_id}
          sortedTabs={sortedTabs.map((tabId) => tabs[tabId]).filter(Boolean)}
          tabs={tabs}
          tabStatuses={statuses}
          teams={[TEAM]}
          planIndex={nextPlanIndex}
          plans={nextPlans}
          activeWork={null}
          providers={providers}
          closingTabIds={new Set()}
          appearingTabIds={new Set()}
          isActiveVisual={true}
          workspaceCanvasActive={false}
          touchMode={true}
          compact={false}
          sidebarActionText="text-xs"
          orchestrators={Object.values(tabs).filter((member) => member.role_profile === "orchestrator")}
          onBeforeOpenPlan={() => {}}
          onActivateTab={onActivateTab}
          onCloseTab={onCloseTab}
          onRenameTab={onRenameTab}
          onNewSession={() => {}}
          newSessionDisabled={newSessionDisabled}
          onRenameTeam={onRenameTeam}
          onAddTeamMember={onAddTeamMember}
          onCloseTeam={onCloseTeam}
        />
      </SidebarDnd>,
    );
  });
  await rerender(plans, planIndex);
  return { host, root, rerender };
}

