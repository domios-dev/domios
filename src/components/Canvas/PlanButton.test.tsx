// Run: npx tsx src/components/Canvas/PlanButton.test.tsx

import { act } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { canvasFocusStore } from "./canvasFocusStore";
import { PlanButton } from "./PlanButton";
import { PlansProvider } from "./PlansProvider";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const API: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: true,
};

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/plan-button" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/api/plans") && (!init || init.method === undefined || init.method === "GET")) {
        const entry = {
          plan_id: FIXTURE_PLAN.id,
          file_path: "/tmp/plan.json",
          owner: FIXTURE_PLAN.owner,
          title: FIXTURE_PLAN.title,
          status: FIXTURE_PLAN.status,
          updated_at: FIXTURE_PLAN.updated_at,
          assigned_pane_ids: [],
        };
        return new Response(JSON.stringify({ plans: [entry] }), { status: 200 });
      }
      if (url.includes(`/api/plans/${FIXTURE_PLAN.id}`)) {
        return new Response(JSON.stringify(FIXTURE_PLAN), { status: 200 });
      }
      return new Response("not found", { status: 404 });
    },
  });
}

let planChangedListener: ((event: { payload?: { plan_id?: string } }) => void) | null = null;

async function installListenMock() {
  const { __setListenForTest } = await import("../../lib/bridge");
  __setListenForTest(async (event, handler) => {
    if (event === "plan-changed") planChangedListener = handler as typeof planChangedListener;
    return () => {
      planChangedListener = null;
    };
  });
}

async function renderButton(paneId: string): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <PlansProvider api={API}>
        <PlanButton paneId={paneId} panes={{}} />
      </PlansProvider>,
    );
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  return { host, root };
}

test("PlanButton hidden when Tab has no matching plan", async () => {
  const { host, root } = await renderButton("unknown-pane");
  assert(!host.querySelector('[data-testid="plan-button"]'), "no button");
  await act(async () => root.unmount());
});

test("PlanButton visible for plan owner Tab id", async () => {
  const { host, root } = await renderButton("orch-1");
  assert(host.querySelector('[data-testid="plan-button"]'), "button visible for owner");
  await act(async () => root.unmount());
});

test("PlanButton opens canvas focus store on click", async () => {
  canvasFocusStore.close();
  const { host, root } = await renderButton("orch-1");
  const btn = host.querySelector('[data-testid="plan-button"]') as HTMLButtonElement;
  assert(btn, "button present");
  await act(async () => btn.click());
  assert(canvasFocusStore.isOpen(), "focus overlay open");
  assert(canvasFocusStore.getTarget()?.planId === FIXTURE_PLAN.id, "focuses fixture plan");
  assert(canvasFocusStore.getTarget()?.focusPaneId === "orch-1", "carries opening Tab for camera routing");
  canvasFocusStore.close();
  await act(async () => root.unmount());
});

test("PlanButton shows a muted activity cue for a changed plan without changing click behavior", async () => {
  const { host, root } = await renderButton("orch-1");
  assert(planChangedListener, "plan change listener installed");
  await act(async () => planChangedListener?.({ payload: { plan_id: FIXTURE_PLAN.id } }));
  assert(canvasFocusStore.anyPulsing([FIXTURE_PLAN.id]), "plan change marks the plan as updated");
  const button = host.querySelector('[data-testid="plan-button"]') as HTMLButtonElement;
  const dots = host.querySelector('[data-testid="plan-button-activity"].activity-dots');
  assert(dots?.classList.contains("activity-dots--small"), "changed plan uses small dots");
  assert(dots?.classList.contains("activity-dots--muted"), "changed plan uses the quiet tone");
  assert(button.querySelector('[role="status"]')?.textContent === "Plan updated", "status text carries the update meaning");
  await act(async () => button.click());
  assert(!canvasFocusStore.anyPulsing([FIXTURE_PLAN.id]), "opening the plan still clears its changed state");
  canvasFocusStore.close();
  await act(async () => root.unmount());
});

test("Batch B pulse keyframes are absent and activity dots retain their static reduced-motion cue", () => {
  const css = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  for (const name of ["ct-plan-sidebar-pulse", "ct-plan-button-pulse", "plan-companion-presence-pulse", "mem-pulse", "mem-pulse-card", "ct-canvas-pulse", "ct-canvas-live", "ct-canvas-heartbeat-dot"]) {
    assert(!css.includes(name), `${name} should be removed`);
  }
  for (const name of ["ct-presence-live", "ct-canvas-step-live"]) {
    assert(!new RegExp(`@keyframes\\s+${name}\\b`).test(css), `${name} should not have a bespoke keyframe`);
  }
  const activityCss = readFileSync(new URL("../../styles/agent-state.css", import.meta.url), "utf8");
  assert(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.activity-dots i\s*\{[^}]*animation:\s*none !important;/s.test(activityCss), "activity dots have a static reduced-motion cue");
});

async function main() {
  setupDom();
  await installListenMock();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} PlanButton tests passed`);
}

main();
