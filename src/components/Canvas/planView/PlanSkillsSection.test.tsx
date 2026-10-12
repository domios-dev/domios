import { configureDaemonFixture } from "../../../test/daemonTransport.testdata";
// Run: npx tsx src/components/Canvas/planView/PlanSkillsSection.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { ApiConfig } from "../../../lib/types";
import { PlanSkillsSection } from "./PlanSkillsSection";
import type { PlanSkillSummary } from "./planSkillsApi";

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
const SKILLS: PlanSkillSummary[] = [
  {
    slug: "deploy",
    title: "Deploy",
    purpose: "Ship to staging",
    check: "echo ok",
    verdict: "keep",
  },
];

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/plan-skills" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
}

setupDom();
configureDaemonFixture();

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

const originalFetch = globalThis.fetch;

test("renders skill rows from API", async () => {
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ skills: SKILLS }), { status: 200 });
  const { host } = await render(
    <PlanSkillsSection planId={FIXTURE_PLAN.id} plan={FIXTURE_PLAN} api={API} />,
  );
  assert(host.querySelector('[data-testid="plan-skill-row"]'), "skill row rendered");
  assert(host.textContent?.includes("Deploy"), "title visible");
  globalThis.fetch = originalFetch;
});

test("opens skill detail on click", async () => {
  globalThis.fetch = async (input) => {
    const url = String(input);
    if (url.includes("/skills/deploy")) {
      return new Response(
        JSON.stringify({ skill: { slug: "deploy", title: "Deploy", body: "# Deploy\n", ledger: [] } }),
        { status: 200 },
      );
    }
    return new Response(JSON.stringify({ skills: SKILLS }), { status: 200 });
  };
  const { host } = await render(
    <PlanSkillsSection planId={FIXTURE_PLAN.id} plan={FIXTURE_PLAN} api={API} />,
  );
  const row = host.querySelector('[data-testid="plan-skill-row"]') as HTMLButtonElement;
  await act(async () => {
    row.click();
    await Promise.resolve();
  });
  assert(host.querySelector('[data-testid="plan-skill-detail"]'), "detail panel opens");
  globalThis.fetch = originalFetch;
});

test("fetch error routes through quiet ErrorState, NEVER prints raw exception", async () => {
  const RAW = "The string did not match the expected pattern.";
  globalThis.fetch = async () => {
    throw new Error(RAW);
  };
  const { host } = await render(
    <PlanSkillsSection planId={FIXTURE_PLAN.id} plan={FIXTURE_PLAN} api={API} />,
  );
  assert(!host.textContent?.includes(RAW), "raw exception string must not render");
  assert(host.querySelector('[data-testid="plan-skills-error"]'), "quiet error state shown");
  assert(host.querySelector('[data-testid="plan-skills-error-retry"]'), "small retry affordance present");
  assert(!host.querySelector('[data-testid="plan-skill-row"]'), "skills list hidden on error");
  globalThis.fetch = originalFetch;
});

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`ok ${name}`);
    } catch (e) {
      failed += 1;
      console.error(`FAIL ${name}:`, e);
    }
  }
  if (failed > 0) process.exit(1);
})();
