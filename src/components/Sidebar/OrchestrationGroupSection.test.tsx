// Run: npx tsx src/components/Sidebar/OrchestrationGroupSection.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestrationGroupSection } from "./OrchestrationGroupSection";
import { RoleBadge } from "./RoleBadge";
import { roleBadgeLabel, shouldShowSectionHeader } from "./orchestrationSections";
import type { Tab } from "../../lib/types";

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
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
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

function tab(overrides: Partial<Tab>): Tab {
  return { id: "p", title: "p", role: null, role_profile: null, ...overrides } as Tab;
}

/** Renders the leadership badges + sections the way the real sidebar composes
 *  an orchestration group, from a seeded tab set. */
function Group({ tabs }: { tabs: Tab[] }) {
  const orch = tabs.find((p) => p.role === "Orchestrator")!;
  const children = tabs.filter((p) => p.id !== orch.id);
  const managers = children.filter((p) => roleBadgeLabel(p) != null);
  const workers = children.filter((p) => roleBadgeLabel(p) == null);
  const orchBadge = roleBadgeLabel(orch);
  const counts = { managers: managers.length, workers: workers.length };
  return (
    <div>
      <div data-testid="group-head">
        {orch.title}
        {orchBadge && <RoleBadge label={orchBadge} title="opus" />}
      </div>
      <OrchestrationGroupSection
        section="manager"
        count={managers.length}
        showHeader={shouldShowSectionHeader("manager", counts)}
      >
        {managers.map((m) => (
          <div key={m.id} data-row={m.id}>
            {m.title}
            {roleBadgeLabel(m) && <RoleBadge label={roleBadgeLabel(m)!} title="fable" />}
          </div>
        ))}
      </OrchestrationGroupSection>
      <OrchestrationGroupSection
        section="worker"
        count={workers.length}
        showHeader={shouldShowSectionHeader("worker", counts)}
      >
        {workers.map((w) => (
          <div key={w.id} data-row={w.id}>
            {w.title}
          </div>
        ))}
      </OrchestrationGroupSection>
    </div>
  );
}

setupDom();

test("seeded group (orch + planner + 2 workers) renders both section headers, planner-first, role badges, no star", async () => {
  const { host, root } = await render(
    <Group
      tabs={[
        tab({ id: "orch", title: "Orchestrator", role: "Orchestrator" }),
        tab({ id: "planner", title: "Planner", role: "Manager", role_profile: "planner" }),
        tab({ id: "w1", title: "Worker A", role: "Worker" }),
        tab({ id: "w2", title: "Worker B", role: "Worker" }),
      ]}
    />,
  );
  assert(host.querySelector('[data-testid="role-badge-orchestrator"]'), "orchestrator badge present");
  assert(host.querySelector('[data-testid="role-badge-planner"]'), "planner badge present");
  assert(host.querySelector('[data-testid="group-section-managers"]'), "Managers header present");
  assert(host.querySelector('[data-testid="group-section-workers"]'), "Workers header present");
  // Managers band precedes Workers band in document order.
  const managersHeader = host.querySelector('[data-testid="group-section-managers"]')!;
  const workersHeader = host.querySelector('[data-testid="group-section-workers"]')!;
  assert(
    managersHeader.compareDocumentPosition(workersHeader) & Node.DOCUMENT_POSITION_FOLLOWING,
    "Managers section comes before Workers section",
  );
  // Planner lives inside the Managers band.
  const managersWrap = host.querySelector('[data-group-section-wrap="manager"]')!;
  assert(managersWrap.querySelector('[data-row="planner"]'), "planner row under Managers");
  assert(host.textContent?.includes("ORCHESTRATOR"), "orchestrator label text");
  assert(host.textContent?.includes("PLANNER"), "planner label text");
  assert(!host.textContent?.includes("★"), "no orchestrator star glyph remains");
  await act(async () => root.unmount());
});

test("one-planner group keeps the MANAGERS header (non-empty section always labeled)", async () => {
  const { host, root } = await render(
    <Group
      tabs={[
        tab({ id: "orch", title: "Orchestrator", role: "Orchestrator" }),
        tab({ id: "planner", title: "Kepler", role: "Manager", role_profile: "planner" }),
      ]}
    />,
  );
  assert(host.querySelector('[data-testid="group-section-managers"]'), "MANAGERS header for a lone planner");
  assert(!host.querySelector('[data-testid="group-section-workers"]'), "no Workers header when no workers");
  // The planner row itself still renders, bare, inside the manager wrap.
  assert(host.querySelector('[data-group-section-wrap="manager"] [data-row="planner"]'), "planner row still present");
  await act(async () => root.unmount());
});

test("lone worker (no planner) keeps the WORKERS header (non-empty section always labeled)", async () => {
  const { host, root } = await render(
    <Group
      tabs={[
        tab({ id: "orch", title: "Orchestrator", role: "Orchestrator" }),
        tab({ id: "w1", title: "Worker A", role: "Worker" }),
      ]}
    />,
  );
  assert(!host.querySelector('[data-testid="group-section-managers"]'), "no Managers header when no managers");
  assert(host.querySelector('[data-testid="group-section-workers"]'), "WORKERS header for a lone worker");
  assert(host.querySelector('[data-group-section-wrap="worker"] [data-row="w1"]'), "worker row still present");
  await act(async () => root.unmount());
});

test("headers RETURN when a section disambiguates: >1 worker, or both sections present", async () => {
  // Two workers, no planner → WORKERS header returns (groups a set), still no Managers.
  const many = await render(
    <Group
      tabs={[
        tab({ id: "orch", title: "Orchestrator", role: "Orchestrator" }),
        tab({ id: "w1", title: "Worker A", role: "Worker" }),
        tab({ id: "w2", title: "Worker B", role: "Worker" }),
      ]}
    />,
  );
  assert(many.host.querySelector('[data-testid="group-section-workers"]'), "WORKERS header returns for >1 worker");
  assert(!many.host.querySelector('[data-testid="group-section-managers"]'), "still no Managers header");
  await act(async () => many.root.unmount());
  // Planner + one worker → both sections present → both headers show.
  const both = await render(
    <Group
      tabs={[
        tab({ id: "orch", title: "Orchestrator", role: "Orchestrator" }),
        tab({ id: "planner", title: "Kepler", role: "Manager", role_profile: "planner" }),
        tab({ id: "w1", title: "Worker A", role: "Worker" }),
      ]}
    />,
  );
  assert(both.host.querySelector('[data-testid="group-section-managers"]'), "MANAGERS header shows when both present");
  assert(both.host.querySelector('[data-testid="group-section-workers"]'), "WORKERS header shows when both present");
  await act(async () => both.root.unmount());
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
  console.log(`OrchestrationGroupSection tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
