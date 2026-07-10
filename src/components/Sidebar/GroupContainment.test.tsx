import { act, useState, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { OrchestrationGroupSection } from "./OrchestrationGroupSection";
import {
  countBySection,
  shouldShowSectionHeader,
  summarizeGroupCounts,
} from "./orchestrationSections";
import { RoleBadge } from "./RoleBadge";
import { presenceColor } from "../Canvas/presence/identity";

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
  const window = new Window({ url: "http://localhost/group-containment" });
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
  return { host, root };
}

interface Member {
  id: string;
  role: "Manager" | "Worker";
  roleProfile: string | null;
}

function GroupBlock({ orchId, members }: { orchId: string; members: Member[] }) {
  const [collapsed, setCollapsed] = useState(false);
  const managers = members.filter((m) => m.role === "Manager");
  const workers = members.filter((m) => m.role === "Worker");
  const counts = countBySection(members);
  const summary = summarizeGroupCounts(counts);
  return (
    <div className="sidebar-orchestration-group" data-orchestration-group={orchId}>
      <div data-testid="group-head" data-orch={orchId}>
        <button
          data-testid="group-collapse-toggle"
          data-collapsed={collapsed ? "true" : "false"}
          onClick={() => setCollapsed((c) => !c)}
        >
          {collapsed ? "▸" : "▾"}
        </button>
        <span>{orchId}</span>
        {collapsed && <span data-testid="group-collapsed-summary">{summary}</span>}
      </div>
      {!collapsed && (
        <div
          data-testid="group-container"
          data-group-container={orchId}
          className="sidebar-group-container"
          style={{ ["--ct-group-presence" as string]: presenceColor(orchId) }}
        >
          <OrchestrationGroupSection
            section="manager"
            count={managers.length}
            showHeader={shouldShowSectionHeader("manager", counts)}
          >
            {managers.map((m) => (
              <div key={m.id} data-row={m.id} className="sidebar-tab">
                {m.id}
                <RoleBadge label="PLANNER" title="fable" />
              </div>
            ))}
          </OrchestrationGroupSection>
          <OrchestrationGroupSection
            section="worker"
            count={workers.length}
            showHeader={shouldShowSectionHeader("worker", counts)}
          >
            {workers.map((w) => (
              <div key={w.id} data-row={w.id} className="sidebar-tab">{w.id}</div>
            ))}
          </OrchestrationGroupSection>
        </div>
      )}
    </div>
  );
}

test("two adjacent groups each render their own containment line in the owner's presence color", async () => {
  const { host, root } = await render(
    <div>
      <GroupBlock
        orchId="planck"
        members={[
          { id: "kepler", role: "Manager", roleProfile: "planner" },
          { id: "bohr", role: "Worker", roleProfile: null },
          { id: "curie", role: "Worker", roleProfile: null },
        ]}
      />
      <GroupBlock
        orchId="euler"
        members={[{ id: "noether", role: "Manager", roleProfile: "planner" }]}
      />
    </div>,
  );
  const containers = host.querySelectorAll('[data-testid="group-container"]');
  assert(containers.length === 2, `one containment block per group (got ${containers.length})`);
  const c0 = containers[0] as HTMLElement;
  const c1 = containers[1] as HTMLElement;
  assert(c0.className.includes("sidebar-group-container"), "block wears the containment class");
  assert(
    c0.style.getPropertyValue("--ct-group-presence") === presenceColor("planck"),
    "first line uses the first owner's presence color",
  );
  assert(
    c1.style.getPropertyValue("--ct-group-presence") === presenceColor("euler"),
    "second line uses the second owner's presence color",
  );
  await act(async () => root.unmount());
});

test("section headers live INSIDE the container, at child level", async () => {
  const { host, root } = await render(
    <GroupBlock
      orchId="planck"
      members={[
        { id: "kepler", role: "Manager", roleProfile: "planner" },
        { id: "bohr", role: "Worker", roleProfile: null },
      ]}
    />,
  );
  const container = host.querySelector('[data-testid="group-container"]')!;
  assert(container.querySelector('[data-testid="group-section-managers"]'), "Managers header inside the block");
  assert(container.querySelector('[data-testid="group-section-workers"]'), "Workers header inside the block");
  assert(container.querySelector('[data-row="kepler"]'), "planner row inside the block");
  assert(container.querySelector('[data-row="bohr"]'), "worker row inside the block");
  await act(async () => root.unmount());
});

test("collapse hides children + headers and shows the count; expand restores", async () => {
  const { host, root } = await render(
    <GroupBlock
      orchId="planck"
      members={[
        { id: "kepler", role: "Manager", roleProfile: "planner" },
        { id: "bohr", role: "Worker", roleProfile: null },
        { id: "curie", role: "Worker", roleProfile: null },
      ]}
    />,
  );
  const toggle = host.querySelector<HTMLButtonElement>('[data-testid="group-collapse-toggle"]')!;
  assert(host.querySelector('[data-testid="group-container"]'), "expanded: container present");
  assert(!host.querySelector('[data-testid="group-collapsed-summary"]'), "expanded: no summary");

  await act(async () => { toggle.click(); });
  assert(!host.querySelector('[data-testid="group-container"]'), "collapsed: container hidden");
  const summary = host.querySelector('[data-testid="group-collapsed-summary"]');
  assert(summary, "collapsed: summary shown");
  assert(summary!.textContent === "1 manager · 2 workers", `count reads "1 manager · 2 workers" (got "${summary!.textContent}")`);

  await act(async () => { toggle.click(); });
  assert(host.querySelector('[data-testid="group-container"]'), "expanded again: container back");
  assert(host.querySelector('[data-row="bohr"]'), "expanded again: worker row back");
  await act(async () => root.unmount());
});

test("one-planner group: head, hairline, MANAGERS header, planner row — headers render for any non-empty section", async () => {
  const { host, root } = await render(
    <GroupBlock
      orchId="planck"
      members={[{ id: "kepler", role: "Manager", roleProfile: "planner" }]}
    />,
  );
  const group = host.querySelector('[data-orchestration-group="planck"]')!;
  assert(group.querySelector('[data-testid="group-head"]'), "element 1: group head");
  const container = group.querySelector('[data-testid="group-container"]')!;
  assert(container, "element 2: containment hairline (container)");
  const rows = container.querySelectorAll("[data-row]");
  assert(rows.length === 1, `element 3: exactly one planner row (got ${rows.length})`);
  assert(group.querySelector('[data-testid="group-section-managers"]'), "MANAGERS header renders even for a lone manager");
  assert(!group.querySelector('[data-testid="group-section-workers"]'), "empty WORKERS section renders no header");
  const boxed = Array.from(group.querySelectorAll("*")).find((el) => {
    const c = (el as HTMLElement).className;
    return typeof c === "string" && (/\bborder\b|\bborder-/.test(c) || /\bbg-/.test(c) || /\brounded\b|\brounded-/.test(c));
  });
  assert(!boxed, `no boxed chrome in the group (found "${boxed ? (boxed as HTMLElement).className : ""}")`);
  assert(!group.querySelector('[data-testid="tab-activity"]'), "no activity dot");
  assert(group.querySelector('[data-testid="role-badge-planner"]'), "planner role text present (borderless)");
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
