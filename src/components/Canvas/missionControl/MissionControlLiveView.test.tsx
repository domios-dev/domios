// Run: npx tsx src/components/Canvas/missionControl/MissionControlLiveView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { MissionControlLiveView } from "./MissionControlLiveView";

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
  const window = new Window({ url: "http://localhost/mission-control-live" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: class {
      observe() {}
      disconnect() {}
    },
  });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "matchMedia", {
    configurable: true,
    value: () => ({
      matches: false,
      media: "",
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  });
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
}

async function render(ui: ReactElement) {
  setupDom();
  const host = document.createElement("div");
  host.style.width = "800px";
  host.style.height = "600px";
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return {
    host,
    root,
    rerender: async (next: ReactElement) => {
      await act(async () => {
        root.render(next);
      });
      await act(async () => {
        await Promise.resolve();
      });
    },
    unmount: async () => {
      await act(async () => {
        root.unmount();
      });
      host.remove();
    },
  };
}

function churn() {
  return window.__codetermMissionControlChurn?.snapshot() ?? null;
}

function identityChurnPlan(plan: Plan): Plan {
  return {
    ...plan,
    canvas: { ...plan.canvas, viewport: plan.canvas.viewport ? { ...plan.canvas.viewport } : null },
    tasks: plan.tasks.map((task) => ({ ...task, tags: [...(task.tags ?? [])] })),
  };
}

test("MissionControlLiveView skips expensive builders when plan/Tabs identity churns but revision is stable", async () => {
  const plan = FIXTURE_PLAN;
  const tabs = {};
  const { rerender, unmount } = await render(
    <MissionControlLiveView plans={[plan]} activePlanId={plan.id} tabs={tabs} />,
  );
  window.__codetermMissionControlChurn?.reset();

  await rerender(<MissionControlLiveView plans={[identityChurnPlan(plan)]} activePlanId={plan.id} tabs={{}} />);
  const afterIdentityChurn = churn();
  assert(afterIdentityChurn?.builds === 0, `buildMissionControl rebuilds: ${afterIdentityChurn?.builds}`);
  assert(afterIdentityChurn?.snapshots === 0, `modelToSnapshot rebuilds: ${afterIdentityChurn?.snapshots}`);
  assert(afterIdentityChurn?.activeWork === 0, `selectActiveWork rebuilds: ${afterIdentityChurn?.activeWork}`);

  const semanticPlan = identityChurnPlan(plan);
  semanticPlan.updated_at += 1;
  await rerender(<MissionControlLiveView plans={[semanticPlan]} activePlanId={semanticPlan.id} tabs={{}} />);
  const afterSemanticChange = churn();
  assert(afterSemanticChange?.builds === 1, `expected one rebuild after semantic change, got ${afterSemanticChange?.builds}`);
  assert(afterSemanticChange?.snapshots === 1, `expected one snapshot rebuild, got ${afterSemanticChange?.snapshots}`);
  assert(afterSemanticChange?.activeWork === 1, `expected one activeWork rebuild, got ${afterSemanticChange?.activeWork}`);

  await unmount();
});

async function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} MissionControlLiveView tests passed`);
}

void main();
