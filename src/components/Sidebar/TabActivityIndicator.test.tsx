// Run: npx tsx src/components/Sidebar/TabActivityIndicator.test.tsx
// EMERGENCE (north-star #2), tab-bar side. A live sidebar over real panes is not
// reliably web-mode drivable, so the per-tab glance is locked here as a component
// test: an owner pane → orchestrator glyph; a directly-assigned pane → worker dot; an
// idle/unrelated pane (and a worker merely nested under an orchestrator) → nothing.
// The resolver stays index-ONLY (the cheap gate that runs for every tab).

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { TabActivityIndicator } from "./TabActivityIndicator";
import { resolvePaneTabActivity } from "../Canvas/paneTabActivity";
import { presenceColor } from "../Canvas/presence/identity";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Pane } from "../../lib/types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkEntry(over: Partial<PlanIndexEntry> = {}): PlanIndexEntry {
  return {
    plan_id: "plan-1",
    file_path: "/tmp/plan-1.json",
    owner: { pane_id: "orch-1", session_id: "sess-orch", title: "Orchestrator" },
    title: "Canvas rendering substrate — Figma-grade migration",
    status: "active",
    updated_at: 100,
    assigned_pane_ids: [],
    task_count: 12,
    ...over,
  };
}

function mkPane(id: string, over: Partial<Pane> = {}): Pane {
  return { id, pty_id: null, pane_type: "Terminal", title: id, cwd: null, ...over } as Pane;
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/tab-activity" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLSpanElement", "Node", "Event", "CustomEvent", "SVGElement"] as const) {
    Object.defineProperty(globalThis, k, { value: (window as never)[k], configurable: true });
  }
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(ui); });
  await act(async () => { await Promise.resolve(); });
  return { host, root };
}

// ── data: resolvePaneTabActivity (index-only gate) ────────────────────────────
test("owner pane → orchestrator activity (plan title + task count)", () => {
  const index = [mkEntry({ assigned_pane_ids: ["w1"] })];
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch", can_spawn: true }) };
  const a = resolvePaneTabActivity(index, "orch-1", panes);
  assert(a?.kind === "orchestrator", `owner classified orchestrator (got ${a?.kind})`);
  assert(a.taskCount === 12, `task count from index (got ${a.taskCount})`);
  assert(a.planTitle.includes("Figma-grade"), "carries plan title for hover");
});

test("directly-assigned pane → worker activity", () => {
  const index = [mkEntry({ assigned_pane_ids: ["w1"] })];
  const panes = {
    "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkPane("w1", { parent_pane_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  const a = resolvePaneTabActivity(index, "w1", panes);
  assert(a?.kind === "worker", `assigned pane classified worker (got ${a?.kind})`);
  assert(a.planId === "plan-1", "worker carries its plan id");
});

test("idle / unrelated pane → no activity (honest)", () => {
  const index = [mkEntry({ assigned_pane_ids: ["w1"] })];
  assert(resolvePaneTabActivity(index, "stranger", {}) === null, "unrelated pane shows nothing");
  assert(resolvePaneTabActivity([], "w1", {}) === null, "empty index → nothing");
});

test("worker nested under orchestrator but NOT assigned → no activity", () => {
  // Chain-related (its parent owns the plan) but no task of its own → not "working".
  const index = [mkEntry({ assigned_pane_ids: [] })];
  const panes = {
    "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkPane("w1", { parent_pane_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  assert(resolvePaneTabActivity(index, "w1", panes) === null, "nested-only pane stays bare");
});

test("session-id ownership survives a pane-id change", () => {
  const index = [mkEntry({ owner: { pane_id: "old-pane", session_id: "sess-orch", title: "Orch" } })];
  const panes = { "orch-2": mkPane("orch-2", { agent_session_id: "sess-orch" }) };
  assert(resolvePaneTabActivity(index, "orch-2", panes)?.kind === "orchestrator", "matches on own session id");
});

test("multi-plan owner counts plans and leads with the freshest", () => {
  const index = [
    mkEntry({ plan_id: "quiet", title: "Quiet", updated_at: 1, task_count: 3 }),
    mkEntry({ plan_id: "busy", title: "Busy", updated_at: 999, task_count: 7 }),
  ];
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) };
  const a = resolvePaneTabActivity(index, "orch-1", panes);
  assert(a?.planId === "busy", `freshest plan leads (got ${a?.planId})`);
  assert(a.planCount === 2, `counts both owned plans (got ${a.planCount})`);
});

// ── view: TabActivityIndicator ────────────────────────────────────────────────
test("orchestrator renders the plan glyph; worker renders the identity dot", async () => {
  const { host, root } = await render(
    <div>
      <TabActivityIndicator activity={{ kind: "orchestrator", planId: "p1", planTitle: "Plan A", taskCount: 4, planCount: 1 }} paneId="orch-1" status="Idle" />
      <TabActivityIndicator activity={{ kind: "worker", planId: "p1", planTitle: "Plan A", taskCount: 4, planCount: 1 }} paneId="w1" status="Idle" />
    </div>,
  );
  const orch = host.querySelector('[data-activity-kind="orchestrator"]');
  const worker = host.querySelector('[data-activity-kind="worker"]');
  assert(orch?.querySelector("svg"), "orchestrator wears a glyph");
  assert(worker, "worker wears a token");
  const dotColor = (worker as HTMLElement).style.background;
  assert(dotColor && dotColor === presenceColor("w1"), `worker dot uses board-identity color (got ${dotColor})`);
  await act(async () => root.unmount());
});

test("working status adds the live pulse; idle does not", async () => {
  const activity = { kind: "worker" as const, planId: "p1", planTitle: "Plan A", taskCount: 1, planCount: 1 };
  const liveR = await render(<TabActivityIndicator activity={activity} paneId="w1" status="Working" />);
  assert(liveR.host.querySelector('[data-activity-kind="worker"]')?.className.includes("ct-presence-live"), "Working → pulse");
  await act(async () => liveR.root.unmount());
  const idleR = await render(<TabActivityIndicator activity={activity} paneId="w1" status="Idle" />);
  assert(!idleR.host.querySelector('[data-activity-kind="worker"]')?.className.includes("ct-presence-live"), "Idle → no pulse");
  await act(async () => idleR.root.unmount());
});

test("+N hover hint appears only with multiple plans", async () => {
  const one = await render(<TabActivityIndicator activity={{ kind: "worker", planId: "p1", planTitle: "Plan A", taskCount: 1, planCount: 1 }} paneId="w1" />);
  assert(!one.host.querySelector('[data-activity-kind="worker"]')?.getAttribute("title")?.includes("+"), "single plan → no +N");
  await act(async () => one.root.unmount());
  const many = await render(<TabActivityIndicator activity={{ kind: "worker", planId: "p1", planTitle: "Plan A", taskCount: 1, planCount: 3 }} paneId="w1" />);
  assert(many.host.querySelector('[data-activity-kind="worker"]')?.getAttribute("title")?.includes("+2 more"), "multi plan → +N more");
  await act(async () => many.root.unmount());
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
