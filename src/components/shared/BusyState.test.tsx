// Run: npx tsx src/components/shared/BusyState.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { RefreshCw } from "lucide-react";
import { BusyState, BusySpinner } from "./BusyState";
import { busyLabel } from "./busyStateLabels";

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
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div") as unknown as HTMLDivElement;
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

test("BusyState renders the canonical three-dot markup", async () => {
  const { host } = await render(<BusyState labelKey="chatPending" />);
  const row = host.querySelector('[data-testid="busy-state"]');
  assert(row, "row must render");
  assert(row!.getAttribute("data-agent-state") === "working", "state=working");
  assert(row!.getAttribute("data-agent-phase") === "thinking", "phase=thinking default");
  const dots = host.querySelectorAll(".ct-phase-dots i");
  assert(dots.length === 3, `expected 3 dots, got ${dots.length}`);
});

test("BusyState label comes from the i18n constants module", async () => {
  const { host } = await render(<BusyState labelKey="planLoad" />);
  const msg = host.querySelector(".ct-agent-msg");
  assert(msg?.textContent === busyLabel("planLoad"), "label from constants");
  assert(busyLabel("planLoad") === "Loading plan", "en value");
});

test("BusySpinner only spins a control's own icon; there is no bare ring", async () => {
  const icon = await render(<BusySpinner icon={RefreshCw} spinning />);
  assert(icon.host.querySelector("svg.animate-spin"), "icon spins");
  assert(!icon.host.querySelector(".ct-busy-ring"), "no ring");
});

test("BusyState explicit label overrides the key (Tab boot stage)", async () => {
  const { host } = await render(<BusyState labelKey="tabBoot" label="Spawning PTY" />);
  const msg = host.querySelector(".ct-agent-msg");
  assert(msg?.textContent === "Spawning PTY", "explicit stage label wins");
});

test("BusyState hideLabel drops the text but keeps the dots + a11y label", async () => {
  const { host } = await render(<BusyState labelKey="chatPending" hideLabel />);
  assert(!host.querySelector(".ct-agent-msg"), "no visible label");
  assert(host.querySelectorAll(".ct-phase-dots i").length === 3, "dots stay");
  const row = host.querySelector('[data-testid="busy-state"]');
  assert(row?.getAttribute("aria-label") === busyLabel("chatPending"), "aria-label present");
  assert(row?.getAttribute("role") === "status", "role=status");
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
      console.error(`  ✗ ${name}: ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n✗ ${failed}/${tests.length} BusyState tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} BusyState tests green`);
}

void main();
