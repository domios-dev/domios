// Run: npx tsx src/components/Canvas/PlansProvider.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { PlanButton } from "./PlanButton";
import { PlansProvider, useFullPlans } from "./PlansProvider";

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

let indexCalls = 0;
let fullCalls = 0;

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/plans-provider" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/api/plans?full=1")) {
        fullCalls++;
        return new Response(JSON.stringify({ plans: [FIXTURE_PLAN] }), { status: 200 });
      }
      if (url.endsWith("/api/plans")) {
        indexCalls++;
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

async function installListenMock() {
  const { __setListenForTest } = await import("../../lib/bridge");
  __setListenForTest(async () => () => {});
}

function FullProbe() {
  useFullPlans();
  return null;
}

async function renderTree(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  return { host, root };
}

test("N PlanButton consumers trigger exactly ONE index fetch", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { host, root } = await renderTree(
    <PlansProvider api={API}>
      <PlanButton paneId="orch-1" panes={{}} />
      <PlanButton paneId="orch-1" panes={{}} />
      <PlanButton paneId="orch-1" panes={{}} />
      <PlanButton paneId="orch-1" panes={{}} />
      <PlanButton paneId="orch-1" panes={{}} />
    </PlansProvider>,
  );
  const buttons = host.querySelectorAll('[data-testid="plan-button"]');
  assert(buttons.length === 5, "five buttons rendered");
  assert(indexCalls === 1, `one index fetch for N consumers (got ${indexCalls})`);
  assert(fullCalls === 0, `no full fetch without an open canvas (got ${fullCalls})`);
  await act(async () => root.unmount());
});

test("full bodies fetched once only when a canvas consumer mounts", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { root } = await renderTree(
    <PlansProvider api={API}>
      <PlanButton paneId="orch-1" panes={{}} />
      <FullProbe />
      <FullProbe />
    </PlansProvider>,
  );
  assert(indexCalls === 1, `one index fetch (got ${indexCalls})`);
  assert(fullCalls === 1, `one batched full fetch for N canvas consumers (got ${fullCalls})`);
  await act(async () => root.unmount());
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
  console.log(`\n✓ ${passed}/${tests.length} PlansProvider tests passed`);
}

main();
