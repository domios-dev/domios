import "../Terminal/ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { RecentlyClosed } from "../Sidebar/RecentlyClosed";
import { RestoredShell } from "../Terminal/RestoredShell";
import { __setInvokeForTest } from "../../lib/bridge";
import type { ClosedStructureView, Tab } from "../../lib/types";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { tabPanelNavigationStore } from "../Terminal/tabPanel/tabPanelNavigation";

const record: ClosedStructureView = {
  id: "close-record", kind: "team", label: "Editors", state: "closed", closed_at: 100,
  undo_until: 700, member_count: 4, team_count: 1, shell_count: 1, agent_count: 3,
  cron_count: 1, blueprint_count: 1,
};

function mount() {
  const host = document.createElement("div");
  document.body.append(host);
  return { host, root: createRoot(host) };
}

test("Recently Closed routes a closed record to one restore handler and hides it once restored", async () => {
  const { host, root } = mount();
  const calls: string[] = [];
  const restore = async (id: string) => { calls.push(id); };
  await act(async () => root.render(<RecentlyClosed views={[]} structures={[record]} showWorkspace={false} onReopen={() => {}} onReopenGroup={restore} />));
  await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="recently-closed"] button')!.click());
  const restoreButton = host.querySelector<HTMLButtonElement>('button[aria-label="Restore Editors"]');
  assert.ok(restoreButton);
  await act(async () => restoreButton.click());
  assert.deepEqual(calls, [record.id]);
  await act(async () => root.render(<RecentlyClosed views={[]} structures={[{ ...record, state: "restored" }]} showWorkspace={false} onReopen={() => {}} onReopenGroup={restore} />));
  assert.equal(host.querySelector('button[aria-label="Restore Editors"]'), null);
  await act(async () => root.unmount());
  host.remove();
});

test("restored utility shell stays stopped on mount and submits one explicit rerun", async () => {
  const { host, root } = mount();
  const calls: [string, unknown][] = [];
  __setInvokeForTest(async (command, args) => { calls.push([command, args]); return undefined as never; });
  const tab = { id: "shell-id", close_restore: { command: "opaque-command", stopped: true, exact_session: false } } as Tab;
  try {
    await act(async () => root.render(<RestoredShell tab={tab} />));
    assert.deepEqual(calls, []);
    const button = host.querySelector<HTMLButtonElement>("button");
    assert.ok(button);
    assert.equal(button.textContent, "Rerun command");
    await act(async () => { button.click(); button.click(); });
    assert.deepEqual(calls, [["mux_rerun_restored_shell", { tabId: tab.id }]]);
  } finally {
    __setInvokeForTest(null);
    await act(async () => root.unmount());
    host.remove();
  }
});

test("original tab identities recover each panel after store reconstruction", () => {
  window.localStorage.clear();
  planSidePanelStore.close();
  planSidePanelStore.resetDocuments();
  const one = { planId: "plan-one", documentsTabId: "original-one", tabView: "changes" as const };
  const two = { planId: "plan-two", documentsTabId: "original-two", tabView: "documents" as const };
  planSidePanelStore.open(one);
  planSidePanelStore.open(two);
  tabPanelNavigationStore.dispatch(one.documentsTabId, { type: "scope", scope: { turnId: "turn-one" } });
  tabPanelNavigationStore.dispatch(one.documentsTabId, { type: "open", view: "changes", id: "/one.ts" });
  tabPanelNavigationStore.dispatch(two.documentsTabId, { type: "open", view: "documents", id: "/two.md" });
  planSidePanelStore.suspend();
  planSidePanelStore.resetCache();
  tabPanelNavigationStore.resetCache();
  planSidePanelStore.activateTab(one.documentsTabId);
  assert.deepEqual(planSidePanelStore.get()?.target, one);
  assert.deepEqual(tabPanelNavigationStore.get(one.documentsTabId).changes, { scope: { turnId: "turn-one" }, detail: "/one.ts", view: "diff" });
  planSidePanelStore.activateTab(two.documentsTabId);
  assert.deepEqual(planSidePanelStore.get()?.target, two);
  assert.equal(tabPanelNavigationStore.get(two.documentsTabId).documents.detail, "/two.md");
  planSidePanelStore.close();
  planSidePanelStore.resetDocuments();
  planSidePanelStore.activateTab(two.documentsTabId);
  assert.equal(planSidePanelStore.get(), null);
  window.localStorage.clear();
  tabPanelNavigationStore.clear();
});
