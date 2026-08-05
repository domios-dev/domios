// Run: npx tsx src/components/Sidebar/DoneView.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { DoneView, completedAgo, isMissingLanding, type CompletedLane } from "./DoneView";

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
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
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

async function click(el: Element | null | undefined) {
  assert(el, "click target must exist");
  await act(async () => {
    (el as HTMLElement).click();
  });
}

async function expandSection(host: HTMLElement) {
  const toggle = host.querySelector('[data-testid="done-view"] > button') as HTMLElement | null;
  await click(toggle);
}

function lane(partial: Partial<CompletedLane> & { worker_tab_id: string }): CompletedLane {
  return {
    title: partial.worker_tab_id,
    status: "done",
    updated_at: 1000,
    ...partial,
  };
}

setupDom();

test("completedAgo formats relative age in seconds, empty for a missing stamp", () => {
  const nowMs = 1_000_000_000_000;
  const nowSecs = nowMs / 1000;
  assert(completedAgo(0, nowMs) === "", "zero stamp -> empty");
  assert(completedAgo(nowSecs - 5, nowMs) === "now", `5s -> now, got ${completedAgo(nowSecs - 5, nowMs)}`);
  assert(completedAgo(nowSecs - 5 * 60, nowMs) === "5m", `5m, got ${completedAgo(nowSecs - 5 * 60, nowMs)}`);
  assert(completedAgo(nowSecs - 3 * 3600, nowMs) === "3h", `3h, got ${completedAgo(nowSecs - 3 * 3600, nowMs)}`);
  assert(completedAgo(nowSecs - 2 * 86400, nowMs) === "2d", `2d, got ${completedAgo(nowSecs - 2 * 86400, nowMs)}`);
});

test("isMissingLanding flags a completed lane with no commit SHA", () => {
  assert(isMissingLanding(lane({ worker_tab_id: "w1", status: "in_review", sha: null })), "no sha -> missing landing");
  assert(!isMissingLanding(lane({ worker_tab_id: "w2", status: "done", sha: "deadbeef" })), "has sha -> not missing");
});

test("section header uses the shared structural label", async () => {
  const { host, root } = await render(<DoneView lanes={[]} />);
  const toggle = host.querySelector('[data-testid="done-view"] > button');
  assert(toggle?.classList.contains("sidebar-structural-header"), "section uses the shared structural header");
  assert(
    toggle?.querySelector(".sidebar-structural-label")?.textContent === "DONE",
    "section is labelled DONE",
  );
  await act(async () => root.unmount());
});

test("renders one accessible row per completed lane with status and short SHA", async () => {
  const { host, root } = await render(
    <DoneView
      lanes={[
        lane({ worker_tab_id: "w1", title: "M3 IV-gate", status: "done", sha: "deadbeefcafe" }),
        lane({ worker_tab_id: "w2", title: "install watchdog", status: "in_review", sha: null }),
      ]}
    />,
  );
  await expandSection(host);

  const list = host.querySelector('[aria-label="Completed lanes"]');
  assert(list?.tagName.toLowerCase() === "ul", "completed lanes render as an accessible list");

  const rows = Array.from(host.querySelectorAll('[data-testid="done-item"] button')) as HTMLElement[];
  assert(rows.length === 2, `expected 2 rows, got ${rows.length}`);
  assert(rows[0].getAttribute("aria-label") === "M3 IV-gate — done", "row exposes an accessible name with status");
  assert(rows[0].textContent?.includes("deadbee"), "row shows the short commit SHA");
  assert(rows[1].getAttribute("aria-label") === "install watchdog — in_review", "second row names its status");

  const noLanding = host.querySelector('[data-testid="done-no-landing"]');
  assert(noLanding?.textContent?.includes("no commit"), "a lane finished with no SHA is flagged at a glance");

  await act(async () => root.unmount());
});

test("empty completed set shows an explicit empty state", async () => {
  const { host, root } = await render(<DoneView lanes={[]} />);
  await expandSection(host);
  const empty = host.querySelector('[data-testid="done-empty"]');
  assert(empty?.textContent?.includes("Nothing finished yet"), "empty state is explicit");
  await act(async () => root.unmount());
});

test("clicking a lane invokes onSelect with that lane", async () => {
  const picked: string[] = [];
  const { host, root } = await render(
    <DoneView
      lanes={[lane({ worker_tab_id: "w9", title: "landed work" })]}
      onSelect={(l) => picked.push(l.worker_tab_id)}
    />,
  );
  await expandSection(host);
  const row = host.querySelector('[data-testid="done-item"] button');
  await click(row);
  assert(picked.length === 1 && picked[0] === "w9", "onSelect receives the clicked lane");
  await act(async () => root.unmount());
});

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`ok - ${name}`);
    } catch (e) {
      failed++;
      console.error(`FAIL - ${name}\n  ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n${failed} test(s) failed`);
    process.exit(1);
  }
  console.log(`\nAll ${tests.length} tests passed`);
})();
