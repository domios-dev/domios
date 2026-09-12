// Run: npx tsx src/components/Canvas/planView/PlanDocumentView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { PERF_SPEC_ID } from "../perfFixtureSections";
import type { Plan } from "../types";
import { PlanPlanView, PlanSpecView } from "./PlanDocumentView";

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
  const window = new Window({ url: "http://localhost/plan-documents" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  setupDom();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  await act(async () => await Promise.resolve());
  return { host, root };
}

const DOCUMENT_PLAN: Plan = {
  ...FIXTURE_PLAN,
  primary_spec_id: PERF_SPEC_ID,
  spec_links: [{ spec_id: PERF_SPEC_ID, spec_section_id: "sp_scale", plan_section_id: "sec-auth" }],
  goal: {
    vector: "Ship an understandable authentication rollout.",
    vision: "A team can read the decision trail without opening a chat.",
    constraints: ["Keep the existing authentication boundary."],
    postulates: ["Every implementation section has a visible reason."],
  },
};

test("Spec renders its title and ordered prose instead of board cards", async () => {
  const { host } = await render(<PlanSpecView mode="focus" planId={DOCUMENT_PLAN.id} plans={[DOCUMENT_PLAN]} />);
  assert(host.querySelector('[data-testid="plan-spec-title"]')?.textContent?.includes("Spec"), "spec title visible");
  assert(host.textContent?.includes("Render at any scale"), "first spec section visible");
  assert(host.textContent?.includes("Plans read like documents"), "later spec section visible");
  assert(!host.querySelector('[data-testid="plan-board-column"]'), "board is not rendered on Spec");
});

test("Plan renders structured goal fields, sections, and Spec provenance", async () => {
  const { host } = await render(
    <PlanPlanView mode="focus" planId={DOCUMENT_PLAN.id} plans={[DOCUMENT_PLAN]} onOpenSpec={() => {}} />,
  );
  assert(host.querySelector('[data-testid="plan-plan-title"]')?.textContent === "Auth rollout", "plan title visible");
  assert(host.textContent?.includes("Ship an understandable authentication rollout."), "goal vector visible");
  assert(host.textContent?.includes("A team can read the decision trail without opening a chat."), "vision visible");
  assert(host.textContent?.includes("Keep the existing authentication boundary."), "constraint visible");
  assert(host.textContent?.includes("Every implementation section has a visible reason."), "postulate visible");
  assert(host.textContent?.includes("Authentication"), "plan section visible");
  assert(host.querySelector('[data-testid="plan-document-spec-link"]')?.textContent?.includes("Render at any scale"), "Spec relation visible");
});

test("Spec and Plan explain empty documents", async () => {
  const emptyPlan: Plan = { ...FIXTURE_PLAN, sections: [], primary_spec_id: null, goal: null };
  const spec = await render(<PlanSpecView mode="focus" planId={emptyPlan.id} plans={[emptyPlan]} />);
  assert(spec.host.querySelector('[data-testid="plan-spec-empty"]'), "Spec empty state visible");
  const specMessage = spec.host.querySelector('[data-testid="plan-spec-empty"]')?.textContent?.trim() ?? "";
  assert(specMessage.length > 0 && specMessage.length < 30, "Spec empty state stays compact");
  const plan = await render(<PlanPlanView mode="focus" planId={emptyPlan.id} plans={[emptyPlan]} />);
  assert(plan.host.querySelector('[data-testid="plan-plan-empty"]'), "Plan empty state visible");
  const planMessage = plan.host.querySelector('[data-testid="plan-plan-empty"]')?.textContent?.trim() ?? "";
  assert(planMessage.length > 0 && planMessage.length < 30, "Plan empty state stays compact");
});

test("both document surfaces expose loading states", async () => {
  const spec = await render(<PlanSpecView mode="workspace" plans={[]} loading />);
  const plan = await render(<PlanPlanView mode="workspace" plans={[]} loading />);
  assert(spec.host.querySelector('[data-testid="plan-spec-loading"]'), "Spec loading state visible");
  assert(plan.host.querySelector('[data-testid="plan-plan-loading"]'), "Plan loading state visible");
});

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`ok ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`FAIL ${name}:`, error);
    }
  }
  if (failed > 0) process.exit(1);
})();
