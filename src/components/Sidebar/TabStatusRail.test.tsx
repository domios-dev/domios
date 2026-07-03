// Run: npx tsx src/components/Sidebar/TabStatusRail.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { TabStatusRail, resolveTabStatusRailVariant } from "./TabStatusRail";
import type { PaneStatus } from "../../lib/types";

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
  const window = new Window({ url: "http://localhost/tab-status-rail" });
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLSpanElement", "Node", "Event"] as const) {
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

// ── resolveTabStatusRailVariant (precedence + idle) ───────────────────────────
test("idle → no rail variant", () => {
  assert(resolveTabStatusRailVariant("Idle", false, false) === null, "idle is absent");
  assert(resolveTabStatusRailVariant(null, false, false) === null, "null is absent");
});

test("working → working variant", () => {
  assert(resolveTabStatusRailVariant("Working", false, false) === "working", "working rail");
});

test("dead beats working and attention", () => {
  assert(resolveTabStatusRailVariant("Dead", true, false) === "dead", "dead wins");
  assert(resolveTabStatusRailVariant("Dead", false, false) === "dead", "dead without attention");
});

test("attention beats working (waiting or pending_prompt flag)", () => {
  assert(resolveTabStatusRailVariant("Waiting", false, false) === "attention", "waiting is attention");
  assert(resolveTabStatusRailVariant("Working", true, false) === "attention", "attention flag wins over working");
});

test("attention still renders when status hasn't hydrated yet (post-reload gap)", () => {
  assert(
    resolveTabStatusRailVariant(null, true, false) === "attention",
    "pending_prompt hydrated ahead of paneStatuses must still show the rail",
  );
});

test("settle overrides idle absence", () => {
  assert(resolveTabStatusRailVariant("Idle", false, true) === "settle", "settle flash while idle");
});

// ── view: TabStatusRail ───────────────────────────────────────────────────────
test("working renders green rail class", async () => {
  const { host, root } = await render(<TabStatusRail status="Working" />);
  const rail = host.querySelector('[data-testid="tab-status-rail"]');
  assert(rail?.getAttribute("data-rail-variant") === "working", "working variant attr");
  assert(rail?.className.includes("ct-tab-status-rail--working"), "working css class");
  await act(async () => root.unmount());
});

test("attention renders yellow rail class", async () => {
  const { host, root } = await render(<TabStatusRail status="Waiting" />);
  const rail = host.querySelector('[data-testid="tab-status-rail"]');
  assert(rail?.getAttribute("data-rail-variant") === "attention", "attention variant");
  assert(rail?.className.includes("ct-tab-status-rail--attention"), "attention css class");
  await act(async () => root.unmount());
});

test("dead renders red rail class", async () => {
  const { host, root } = await render(<TabStatusRail status="Dead" />);
  const rail = host.querySelector('[data-testid="tab-status-rail"]');
  assert(rail?.getAttribute("data-rail-variant") === "dead", "dead variant");
  assert(rail?.className.includes("ct-tab-status-rail--dead"), "dead css class");
  await act(async () => root.unmount());
});

test("idle renders no rail", async () => {
  const { host, root } = await render(<TabStatusRail status="Idle" />);
  assert(!host.querySelector('[data-testid="tab-status-rail"]'), "idle has no rail");
  await act(async () => root.unmount());
});

test("rail is aria-hidden", async () => {
  const { host, root } = await render(<TabStatusRail status="Working" />);
  assert(host.querySelector('[data-testid="tab-status-rail"]')?.getAttribute("aria-hidden") === "true", "aria-hidden");
  await act(async () => root.unmount());
});

test("mount with idle does not fire settle flash", async () => {
  const { host, root } = await render(<TabStatusRail status="Idle" />);
  assert(!host.querySelector('[data-rail-variant="settle"]'), "no settle on mount");
  await act(async () => root.unmount());
});

test("working→idle edge fires settle once then clears", async () => {
  const timers = new Map<number, { fn: () => void; delay: number }>();
  let nextId = 1;
  const origSetTimeout = globalThis.setTimeout.bind(globalThis);
  const origClearTimeout = globalThis.clearTimeout.bind(globalThis);
  (globalThis as { setTimeout: typeof setTimeout }).setTimeout = ((fn: () => void, delay?: number) => {
    const id = nextId++;
    timers.set(id, { fn, delay: delay ?? 0 });
    return id as unknown as ReturnType<typeof setTimeout>;
  }) as typeof setTimeout;
  (globalThis as { clearTimeout: typeof clearTimeout }).clearTimeout = ((id: number) => {
    timers.delete(id);
  }) as typeof clearTimeout;

  let status: PaneStatus = "Working";
  const { host, root } = await render(<TabStatusRail status={status} />);
  assert(host.querySelector('[data-rail-variant="working"]'), "starts working");

  await act(async () => {
    status = "Idle";
    root.render(<TabStatusRail status={status} />);
    await Promise.resolve();
  });
  assert(host.querySelector('[data-rail-variant="settle"]'), "settle after Working→Idle");

  await act(async () => {
    for (const [id, t] of timers) {
      if (t.delay === 900) {
        t.fn();
        timers.delete(id);
      }
    }
    await Promise.resolve();
  });
  assert(!host.querySelector('[data-testid="tab-status-rail"]'), "rail gone after settle timeout");

  await act(async () => root.unmount());
  globalThis.setTimeout = origSetTimeout;
  globalThis.clearTimeout = origClearTimeout;
});

test("dead→idle edge does not fire settle", async () => {
  let status: PaneStatus = "Dead";
  const { host, root } = await render(<TabStatusRail status={status} />);
  await act(async () => {
    status = "Idle";
    root.render(<TabStatusRail status={status} />);
    await Promise.resolve();
  });
  assert(!host.querySelector('[data-rail-variant="settle"]'), "no settle for dead→idle");
  await act(async () => root.unmount());
});

test("no IS_WEB references in TabStatusRail module", async () => {
  const fs = await import("node:fs");
  const path = await import("node:path");
  const src = fs.readFileSync(
    path.join(process.cwd(), "src/components/Sidebar/TabStatusRail.tsx"),
    "utf8",
  );
  assert(!src.includes("IS_WEB"), "TabStatusRail stays platform-blind");
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
