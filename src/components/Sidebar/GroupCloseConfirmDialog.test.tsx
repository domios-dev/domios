// Run: npx tsx src/components/Sidebar/GroupCloseConfirmDialog.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { GroupCloseConfirmDialog } from "./GroupCloseConfirmDialog";
import type { OrchestrationGroupMember } from "./orchestrationGroupClose";

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
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
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

function member(
  id: string,
  role: OrchestrationGroupMember["role"],
  roleProfile: string | null,
): OrchestrationGroupMember {
  return { tabId: id, title: id, role, roleProfile };
}

setupDom();

test("summary counts BY SECTION — 1 manager, N workers", async () => {
  const { root } = await render(
    <GroupCloseConfirmDialog
      orchestratorTitle="Orchestrator"
      members={[
        member("planner", "Manager", "planner"),
        member("w1", "Worker", null),
        member("w2", "Worker", "worker"),
      ]}
      onConfirm={() => {}}
      onCancel={() => {}}
    />,
  );
  const summary = document.querySelector('[data-testid="group-close-summary"]');
  assert(summary, "summary line present");
  assert(summary!.textContent?.includes("1 manager"), `singular manager: ${summary!.textContent}`);
  assert(summary!.textContent?.includes("2 workers"), `plural workers: ${summary!.textContent}`);
  assert(summary!.textContent?.includes("4 Tabs"), "total Tabs = orchestrator + 3 members");
  await act(async () => root.unmount());
});

test("worker-only group omits the managers clause", async () => {
  const { root } = await render(
    <GroupCloseConfirmDialog
      orchestratorTitle="Orchestrator"
      members={[member("w1", "Worker", null), member("w2", "Worker", null)]}
      onConfirm={() => {}}
      onCancel={() => {}}
    />,
  );
  const summary = document.querySelector('[data-testid="group-close-summary"]');
  assert(summary!.textContent?.includes("2 workers"), "workers counted");
  assert(!summary!.textContent?.includes("manager"), "no managers clause");
  await act(async () => root.unmount());
});

test("Team mode names the Team action and counts only Team tabs", async () => {
  const { root } = await render(
    <GroupCloseConfirmDialog
      mode="team"
      orchestratorTitle="QA fixtures"
      members={[member("manager", "Orchestrator", null), member("worker", "Worker", null)]}
      onConfirm={() => {}}
      onCancel={() => {}}
    />,
  );
  assert(document.body.textContent?.includes("Close Team?"), "Team confirmation has Team title");
  assert(document.body.textContent?.includes("all 2 Team tabs"), "Team count excludes synthetic root");
  assert(document.querySelector('[data-testid="group-close-confirm-button"]')?.textContent?.includes("Close Team"), "Team confirm action is explicit");
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`GroupCloseConfirmDialog tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
