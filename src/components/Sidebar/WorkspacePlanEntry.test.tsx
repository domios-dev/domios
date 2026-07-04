
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { WorkspacePlanEntry, WorkspacePlanTreeSlot } from "./WorkspacePlanEntry";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Pane, Workspace } from "../../lib/types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const PLAN_TITLE = "Canvas rendering substrate — Figma-grade migration";

function mkEntry(over: Partial<PlanIndexEntry> = {}): PlanIndexEntry {
  return {
    plan_id: "plan-1",
    file_path: "/repo/hub/.codeterm/plans/active/plan-1/plan.json",
    owner: { pane_id: "orch-1", session_id: "sess-orch", title: "Orchestrator" },
    title: PLAN_TITLE,
    status: "active",
    updated_at: 100,
    assigned_pane_ids: [],
    task_count: 12,
    section_count: 1,
    ...over,
  };
}

const workspace: Workspace = {
  id: "ws-1",
  name: "Hub",
  cwd: "/repo/hub",
  active_tab_id: "tab-1",
  tabs: [{ id: "tab-1", pane_id: "orch-1" }],
};
const panes: Record<string, Pane> = {
  "orch-1": { id: "orch-1", pty_id: null, pane_type: "Terminal", title: "Orch", cwd: null } as Pane,
};

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/workspace-plan" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLSpanElement", "Node", "Event", "CustomEvent", "MouseEvent", "SVGElement"] as const) {
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

const slot = (planIndex: PlanIndexEntry[]) => (
  <WorkspacePlanTreeSlot workspace={workspace} panes={panes} planIndex={planIndex} canvasActive={false} />
);

test("present → tab-row plan entry with plan title + task hint", async () => {
  const { host, root } = await render(slot([mkEntry()]));
  const btn = host.querySelector('[data-testid="workspace-plan-entry"]');
  assert(btn, "entry renders for a matching plan");
  assert(btn.getAttribute("data-pinned") === "true", "entry is pinned, not a tab");
  assert(btn.className.includes("sidebar-tab"), "uses tab row grammar");
  assert(btn.className.includes("sidebar-tab-plan"), "keeps distinct plan identity");
  assert(btn.getAttribute("data-active") === "false", "inactive until canvas opens");
  assert(btn.querySelector("svg"), "wears the plan/map glyph (visually distinct)");
  const title = host.querySelector('[data-testid="workspace-plan-title"]');
  assert(title?.textContent?.includes("Figma-grade"), `shows the real plan title (got ${title?.textContent})`);
  assert(title?.className.includes("truncate"), "title uses ellipsis truncation");
  assert(title?.className.includes("whitespace-nowrap"), "title avoids letter-shard wrapping");
  const hint = host.querySelector('[data-testid="workspace-plan-hint"]');
  assert(hint?.textContent === "12 tasks", `shows the task hint (got ${hint?.textContent})`);
  assert(!host.querySelector('[data-testid="workspace-plan-more"]'), "single plan → no +N");
  await act(async () => root.unmount());
});

test("active plan entry sets data-active and shows selection pill", async () => {
  const { host, root } = await render(
    <WorkspacePlanEntry entries={[mkEntry()]} isActive onClick={() => {}} />,
  );
  const btn = host.querySelector('[data-testid="workspace-plan-entry"]');
  assert(btn?.getAttribute("data-active") === "true", "active state exposed for sidebar tests");
  assert(btn?.getAttribute("aria-pressed") === "true", "pressed for a11y");
  const pill = btn?.querySelector(".pointer-events-none.absolute");
  assert(pill, "active pill like session tabs");
  await act(async () => root.unmount());
});

test("in-flight badge renders when count is positive", async () => {
  const { host, root } = await render(
    <WorkspacePlanEntry entries={[mkEntry()]} isActive={false} inflightCount={3} onClick={() => {}} />,
  );
  const badge = host.querySelector('[data-testid="sidebar-inflight-badge"]');
  assert(badge?.textContent === "3", "badge shows inflight count");
  await act(async () => root.unmount());
});

test("long plan title stays in a single truncated line", async () => {
  const longTitle =
    "Canvas rendering substrate — Figma-grade migration with extra words that should not shard";
  const { host, root } = await render(
    <WorkspacePlanEntry
      entries={[mkEntry({ title: longTitle })]}
      isActive={false}
      onClick={() => {}}
    />,
  );
  const title = host.querySelector('[data-testid="workspace-plan-title"]') as HTMLSpanElement | null;
  assert(title?.textContent === longTitle, "full title in DOM for tooltip/ellipsis source");
  assert(title?.className.includes("truncate"), "long title uses truncate");
  assert(title?.className.includes("whitespace-nowrap"), "long title does not wrap per-letter");
  await act(async () => root.unmount());
});

test("one click opens that plan's canvas", async () => {
  canvasFocusStore.closeWorkspace();
  const { host, root } = await render(slot([mkEntry()]));
  const btn = host.querySelector('[data-testid="workspace-plan-entry"]') as HTMLButtonElement;
  await act(async () => { btn.click(); });
  assert(canvasFocusStore.isWorkspaceOpen(), "click opened the workspace canvas");
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === "plan-1", "opened THIS plan");
  canvasFocusStore.closeWorkspace();
  await act(async () => root.unmount());
});

test("multiple plans → no +N badge, still ambiguous (no single plan id)", async () => {
  canvasFocusStore.closeWorkspace();
  const index = [
    mkEntry({ plan_id: "quiet", title: "Quiet plan", updated_at: 1 }),
    mkEntry({ plan_id: "busy", title: "Busy plan", file_path: "/repo/hub/.codeterm/plans/active/busy/plan.json", updated_at: 999 }),
  ];
  const { host, root } = await render(slot(index));
  assert(!host.querySelector('[data-testid="workspace-plan-more"]'), "+N badge removed from UI");
  const title = host.querySelector('[data-testid="workspace-plan-title"]');
  assert(title?.textContent === "Busy plan", `leads with the freshest plan (got ${title?.textContent})`);
  const btn = host.querySelector('[data-testid="workspace-plan-entry"]') as HTMLButtonElement;
  await act(async () => { btn.click(); });
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === undefined, "multi-plan open stays plan-ambiguous");
  canvasFocusStore.closeWorkspace();
  await act(async () => root.unmount());
});

test("plan entry shows open task count when available, falls back to total", async () => {
  const { host: hostOpen, root: rootOpen } = await render(slot([mkEntry({ task_count: 12, open_task_count: 7 })]));
  const hintOpen = hostOpen.querySelector('[data-testid="workspace-plan-hint"]');
  assert(hintOpen?.textContent === "7 tasks", `open_task_count shown (got ${hintOpen?.textContent})`);
  await act(async () => rootOpen.unmount());

  const { host: hostFallback, root: rootFallback } = await render(slot([mkEntry({ task_count: 12 })]));
  const hintFallback = hostFallback.querySelector('[data-testid="workspace-plan-hint"]');
  assert(hintFallback?.textContent === "12 tasks", `falls back to task_count (got ${hintFallback?.textContent})`);
  await act(async () => rootFallback.unmount());
});

test("plan entry does not show 'Plan' label text", async () => {
  const { host, root } = await render(slot([mkEntry()]));
  const entry = host.querySelector('[data-testid="workspace-plan-entry"]');
  assert(entry, "entry present");
  const kindSpan = entry?.querySelector(".sidebar-workspace-plan-kind");
  assert(!kindSpan, "'Plan' kind label span removed from entry");
  await act(async () => root.unmount());
});

test("no plans → renders nothing (graceful empty index)", async () => {
  const { host, root } = await render(slot([]));
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "empty index → no entry");
  assert(host.childElementCount === 0, "slot collapses to nothing");
  await act(async () => root.unmount());
});

test("compact mode → glyph-only button, still pinned and clickable", async () => {
  const calls: number[] = [];
  const { host, root } = await render(
    <WorkspacePlanEntry entries={[mkEntry()]} isActive={false} compact onClick={() => calls.push(1)} />,
  );
  const btn = host.querySelector('[data-testid="workspace-plan-entry"]') as HTMLButtonElement;
  assert(btn?.className.includes("sidebar-workspace-plan-compact"), "compact variant");
  assert(btn.querySelector("svg"), "compact still shows the glyph");
  assert(!host.querySelector('[data-testid="workspace-plan-title"]'), "compact hides the title row");
  await act(async () => { btn.click(); });
  assert(calls.length === 1, "compact entry is clickable");
  await act(async () => root.unmount());
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
