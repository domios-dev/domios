// Run: npx tsx src/components/Sidebar/RecentlyClosed.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { RecentlyClosed, groupClosedViews, describeGroup } from "./RecentlyClosed";
import { reopenClosedPane, reopenClosedGroup, __setInvokeForTest } from "../../lib/bridge";
import type { ClosedPaneView } from "../../lib/types";

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
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
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

async function click(el: Element | null | undefined) {
  assert(el, "click target must exist");
  await act(async () => {
    (el as HTMLElement).click();
  });
}

async function expandSection(host: HTMLElement) {
  const toggle = host.querySelector('[data-testid="recently-closed"] button') as HTMLElement | null;
  await click(toggle);
}

/** A flat newest-first view. `single` panes each get their own group id; pass
 *  explicit group fields for cascade/workspace fixtures. */
function view(partial: Partial<ClosedPaneView> & { pane_id: string }): ClosedPaneView {
  return {
    title: partial.pane_id,
    closed_at: 1000,
    backing: { kind: "pty" },
    index: 0,
    orphaned: false,
    group_id: partial.pane_id,
    group_kind: "single",
    workspace_id: "ws1",
    ...partial,
  };
}

setupDom();

test("groupClosedViews collapses by group_id preserving first-seen order", () => {
  const groups = groupClosedViews([
    view({ pane_id: "s1" }),
    view({ pane_id: "c-orch", group_id: "g-cas", group_kind: "cascade" }),
    view({ pane_id: "c-w1", group_id: "g-cas", group_kind: "cascade", parent_pane_id: "c-orch" }),
    view({ pane_id: "s2" }),
  ]);
  assert(groups.length === 3, `expected 3 groups, got ${groups.length}`);
  assert(groups[0].kind === "single" && groups[0].id === "s1", "first group is single s1");
  assert(groups[1].kind === "cascade" && groups[1].panes.length === 2, "cascade collapses its two panes");
  assert(groups[2].id === "s2", "order preserved by first appearance");
});

test("describeGroup labels cascade and workspace groups", () => {
  const cascade = groupClosedViews([
    view({ pane_id: "orch", title: "Orchestrator", group_id: "g", group_kind: "cascade" }),
    view({ pane_id: "w1", group_id: "g", group_kind: "cascade", parent_pane_id: "orch" }),
    view({ pane_id: "w2", group_id: "g", group_kind: "cascade", parent_pane_id: "orch" }),
    view({ pane_id: "w3", group_id: "g", group_kind: "cascade", parent_pane_id: "orch" }),
  ])[0];
  assert(describeGroup(cascade) === "Orchestrator + 3 workers", `cascade label: ${describeGroup(cascade)}`);

  const ws = groupClosedViews([
    view({ pane_id: "t1", group_id: "w", group_kind: "workspace", workspace_name: "General" }),
    view({ pane_id: "t2", group_id: "w", group_kind: "workspace", workspace_name: "General" }),
  ])[0];
  assert(describeGroup(ws) === 'Workspace "General" — 2 tabs', `workspace label: ${describeGroup(ws)}`);
});

test("renders a row per single close with title and model", async () => {
  const { host, root } = await render(
    <RecentlyClosed
      views={[
        view({ pane_id: "p1", title: "claude — fix bug", model: "Opus" }),
        view({ pane_id: "p2", title: "build logs" }),
      ]}
      showWorkspace={false}
      onReopen={() => {}}
      onReopenGroup={() => {}}
    />,
  );
  await expandSection(host);

  const rows = Array.from(host.querySelectorAll('[data-testid="recently-closed-item"]'));
  assert(rows.length === 2, `expected 2 rows, got ${rows.length}`);
  assert(rows[0].textContent?.includes("claude — fix bug"), "first row shows its title");
  assert(rows[0].textContent?.includes("Opus"), "first row shows its model");
  assert(host.querySelectorAll('[data-testid="recently-closed-group"]').length === 0, "no group rows for singles");

  await act(async () => root.unmount());
});

test("clicking a single reopens that index via the bridge", async () => {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    return { id: "t9", title: "x", pane_id: "p9" } as T;
  });
  const { host, root } = await render(
    <RecentlyClosed
      views={[
        view({ pane_id: "p1", title: "newest", index: 0 }),
        view({ pane_id: "p2", title: "older", index: 1 }),
      ]}
      showWorkspace={false}
      onReopen={(wsId, idx) => { void reopenClosedPane(wsId, idx); }}
      onReopenGroup={() => {}}
    />,
  );
  await expandSection(host);

  const rows = Array.from(host.querySelectorAll('[data-testid="recently-closed-item"]')) as HTMLElement[];
  await click(rows[1]);

  const reopen = calls.find((c) => c.cmd === "mux_reopen_closed_pane");
  assert(reopen, "clicking should invoke mux_reopen_closed_pane");
  assert(reopen.args?.workspaceId === "ws1", `expected workspaceId ws1, got ${String(reopen.args?.workspaceId)}`);
  assert(reopen.args?.index === 1, `expected index 1 (the clicked entry), got ${String(reopen.args?.index)}`);

  await act(async () => root.unmount());
  __setInvokeForTest(null);
});

test("a cascade renders as one collapsed group entry, distinct from singles", async () => {
  const { host, root } = await render(
    <RecentlyClosed
      views={[
        view({ pane_id: "orch", title: "Orchestrator", group_id: "g", group_kind: "cascade" }),
        view({ pane_id: "w1", group_id: "g", group_kind: "cascade", parent_pane_id: "orch" }),
        view({ pane_id: "w2", group_id: "g", group_kind: "cascade", parent_pane_id: "orch" }),
      ]}
      showWorkspace={false}
      onReopen={() => {}}
      onReopenGroup={() => {}}
    />,
  );
  await expandSection(host);

  const groupEls = host.querySelectorAll('[data-testid="recently-closed-group"]');
  assert(groupEls.length === 1, `expected 1 group entry, got ${groupEls.length}`);
  const restore = host.querySelector('[data-testid="recently-closed-group-restore"]');
  assert(restore?.textContent?.includes("Orchestrator + 2 workers"), `group label: ${restore?.textContent}`);
  // collapsed by default — child panes not rendered
  assert(
    host.querySelectorAll('[data-testid="recently-closed-item"]').length === 0,
    "children hidden until the group is expanded",
  );

  await act(async () => root.unmount());
});

test("clicking the group header reopens the whole group via mux_reopen_closed_group", async () => {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    return { workspace_id: "ws1", tab_id: "t1", pane_id: "orch" } as T;
  });
  const { host, root } = await render(
    <RecentlyClosed
      views={[
        view({ pane_id: "orch", title: "Orchestrator", group_id: "g-cas", group_kind: "cascade" }),
        view({ pane_id: "w1", group_id: "g-cas", group_kind: "cascade", parent_pane_id: "orch" }),
      ]}
      showWorkspace={false}
      onReopen={() => {}}
      onReopenGroup={(groupId) => { void reopenClosedGroup(groupId); }}
    />,
  );
  await expandSection(host);

  await click(host.querySelector('[data-testid="recently-closed-group-restore"]'));

  const reopen = calls.find((c) => c.cmd === "mux_reopen_closed_group");
  assert(reopen, "clicking the group header should invoke mux_reopen_closed_group");
  assert(reopen.args?.groupId === "g-cas", `expected groupId g-cas, got ${String(reopen.args?.groupId)}`);

  await act(async () => root.unmount());
  __setInvokeForTest(null);
});

test("expanding a group reveals individual panes, each reopenable on its own", async () => {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    return { id: "t9", title: "x", pane_id: "p9" } as T;
  });
  const { host, root } = await render(
    <RecentlyClosed
      views={[
        view({ pane_id: "orch", title: "Orchestrator", group_id: "g", group_kind: "cascade", index: 0 }),
        view({ pane_id: "w1", title: "worker one", group_id: "g", group_kind: "cascade", parent_pane_id: "orch", index: 1 }),
      ]}
      showWorkspace={false}
      onReopen={(wsId, idx) => { void reopenClosedPane(wsId, idx); }}
      onReopenGroup={() => {}}
    />,
  );
  await expandSection(host);

  await click(host.querySelector('[data-testid="recently-closed-group-toggle"]'));
  const children = Array.from(host.querySelectorAll('[data-testid="recently-closed-item"]')) as HTMLElement[];
  assert(children.length === 2, `expected 2 child panes after expand, got ${children.length}`);
  assert(children[1].textContent?.includes("worker one"), "child shows the individual pane title");

  await click(children[1]);
  const reopen = calls.find((c) => c.cmd === "mux_reopen_closed_pane");
  assert(reopen, "clicking a child should invoke the single reopen path");
  assert(reopen.args?.index === 1, `expected child index 1, got ${String(reopen.args?.index)}`);

  await act(async () => root.unmount());
  __setInvokeForTest(null);
});

test("renders nothing when there are no recently-closed entries", async () => {
  const { host: empty, root: r1 } = await render(
    <RecentlyClosed views={[]} showWorkspace={false} onReopen={() => {}} onReopenGroup={() => {}} />,
  );
  assert(!empty.querySelector('[data-testid="recently-closed"]'), "empty ring renders no panel");
  await act(async () => r1.unmount());
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
  console.log(`RecentlyClosed tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
