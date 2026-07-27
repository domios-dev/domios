// Run: npx tsx src/components/shared/BusyState.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { RefreshCw } from "lucide-react";
import { BusyState, BusySpinner } from "./BusyState";
import { busyLabel } from "./busyStateLabels";

const here = dirname(fileURLToPath(import.meta.url));

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
  assert(host.querySelector(".activity-dots--medium"), "block rows default to medium dots");
});

test("BusyState selects dot size from context and accepts an explicit override", async () => {
  const inline = await render(<BusyState inline />);
  assert(inline.host.querySelector(".activity-dots--small"), "inline rows default to small dots");

  const overridden = await render(<BusyState inline dotSize="micro" />);
  assert(overridden.host.querySelector(".activity-dots--micro"), "explicit dot size wins");
});

test("BusyState label comes from the i18n constants module", async () => {
  const { host } = await render(<BusyState labelKey="planLoad" />);
  const msg = host.querySelector(".ct-agent-msg");
  assert(msg?.textContent === busyLabel("planLoad"), "label from constants");
  assert(busyLabel("planLoad") === "Loading plan", "en value");
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

test("BusySpinner icon form carries the reduced-motion opt-out class", async () => {
  const { host } = await render(<BusySpinner icon={RefreshCw} spinning />);
  const svg = host.querySelector("svg");
  assert(svg, "icon renders");
  const cls = svg!.getAttribute("class") ?? "";
  assert(cls.includes("animate-spin"), "spins when spinning");
  assert(cls.includes("ct-busy-spinner"), "reduced-motion hook class");
});

test("BusySpinner icon form does not spin when spinning=false", async () => {
  const { host } = await render(<BusySpinner icon={RefreshCw} spinning={false} />);
  const cls = host.querySelector("svg")?.getAttribute("class") ?? "";
  assert(!cls.includes("animate-spin"), "static when not spinning");
});

test("BusySpinner ring form renders a bare spinning ring", async () => {
  const { host } = await render(<BusySpinner data-testid="ring" />);
  const ring = host.querySelector('[data-testid="ring"]');
  assert(ring, "ring renders");
  const cls = ring!.getAttribute("class") ?? "";
  assert(cls.includes("ct-busy-ring"), "has ring class");
  assert(cls.includes("animate-spin"), "spins by default");
});

test("agent-state.css neutralizes motion under prefers-reduced-motion", () => {
  const css = readFileSync(join(here, "..", "..", "styles", "agent-state.css"), "utf8");
  const block = css.slice(css.indexOf("prefers-reduced-motion"));
  assert(block.includes(".ct-phase-dots i"), "dots covered by reduced-motion");
  assert(block.includes(".ct-busy-spinner"), "spinner covered by reduced-motion");
  assert(block.includes("animation: none"), "reduced-motion kills animation");
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
