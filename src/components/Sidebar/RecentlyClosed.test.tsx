// Run: npx tsx src/components/Sidebar/RecentlyClosed.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { RecentlyClosed } from "./RecentlyClosed";
import { reopenClosedPane } from "../../lib/bridge";
import { __setInvokeForTest } from "../../lib/bridge";
import type { ClosedPane } from "../../lib/types";

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

async function expand(host: HTMLElement) {
  const toggle = host.querySelector('[data-testid="recently-closed"] button') as HTMLElement | null;
  if (toggle) {
    await act(async () => {
      toggle.click();
    });
  }
}

function makeEntries(panes: ClosedPane[], wsId = "ws1") {
  return panes.map((entry, index) => ({ entry, workspaceId: wsId, index, orphaned: false }));
}

setupDom();

test("renders a row per recently-closed entry with title and model", async () => {
  const entries = makeEntries([
    { pane_id: "p1", title: "claude — fix bug", model: "Opus", closed_at: 1000, backing: { kind: "pty" } },
    { pane_id: "p2", title: "build logs", closed_at: 900, backing: { kind: "pty" } },
  ]);
  const { host, root } = await render(
    <RecentlyClosed entries={entries} showWorkspace={false} onReopen={() => {}} />,
  );
  await expand(host);

  const rows = Array.from(host.querySelectorAll('[data-testid="recently-closed-item"]'));
  assert(rows.length === 2, `expected 2 rows, got ${rows.length}`);
  assert(rows[0].textContent?.includes("claude — fix bug"), "first row shows its title");
  assert(rows[0].textContent?.includes("Opus"), "first row shows its model");
  assert(rows[1].textContent?.includes("build logs"), "second row shows its title");

  await act(async () => root.unmount());
});

test("clicking an entry reopens that index via the bridge", async () => {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  __setInvokeForTest(async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    return { id: "t9", title: "x", pane_id: "p9" } as T;
  });
  const entries = makeEntries([
    { pane_id: "p1", title: "newest", closed_at: 1000, backing: { kind: "pty" } },
    { pane_id: "p2", title: "older", closed_at: 900, backing: { kind: "pty" } },
  ]);
  const { host, root } = await render(
    <RecentlyClosed
      entries={entries}
      showWorkspace={false}
      onReopen={(wsId, idx) => { void reopenClosedPane(wsId, idx); }}
    />,
  );
  await expand(host);

  const rows = Array.from(host.querySelectorAll('[data-testid="recently-closed-item"]')) as HTMLElement[];
  await act(async () => {
    rows[1].click();
  });

  const reopen = calls.find((c) => c.cmd === "mux_reopen_closed_pane");
  assert(reopen, "clicking should invoke mux_reopen_closed_pane");
  assert(reopen.args?.workspaceId === "ws1", `expected workspaceId ws1, got ${String(reopen.args?.workspaceId)}`);
  assert(reopen.args?.index === 1, `expected index 1 (the clicked entry), got ${String(reopen.args?.index)}`);

  await act(async () => root.unmount());
  __setInvokeForTest(null);
});

test("renders nothing when there are no recently-closed entries", async () => {
  const { host: empty, root: r1 } = await render(
    <RecentlyClosed entries={[]} showWorkspace={false} onReopen={() => {}} />,
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
