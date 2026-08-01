
// Run: npx tsx src/components/shared/Button.test.tsx

import React, { act } from "react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { Button } from "./Button";

const globalsCss = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../../styles/globals.css"),
  "utf8",
);
const tokensCss = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../../styles/tokens.css"),
  "utf8",
);

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) { tests.push([name, fn]); }
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
  await act(async () => { root.render(ui); });
  return { host, root };
}

test("Button primary has ct-btn-primary class", async () => {
  const { host } = await render(<Button variant="primary">Save</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-primary"), `expected ct-btn-primary in ${btn.className}`);
  assert(btn.className.includes("ct-liquid-interactive"), "shared interaction class missing");
  assert(
    /\.ct-btn-primary\s*\{[^}]*color:\s*var\(--ct-on-accent,\s*#fff\)[^}]*font-weight:\s*600/s.test(
      globalsCss,
    ),
    "primary buttons must use the theme's strong on-accent text",
  );
});

test("liquid interaction contract is bounded, explicit, and motion-safe", () => {
  assert(/--ct-duration-liquid-hover:\s*380ms/.test(tokensCss), "hover duration token missing");
  assert(/--ct-duration-liquid-press:\s*280ms/.test(tokensCss), "press duration token missing");
  const rule = globalsCss.match(/\.ct-liquid-interactive\s*\{([^}]*)\}/s)?.[1] ?? "";
  assert(/transform-origin:\s*center/.test(rule), "interaction must originate at center");
  assert(
    /transition:\s*transform[^;]*,\s*color[^;]*,\s*background-color[^;]*,\s*border-color/s.test(rule),
    "interaction must transition only explicit visual properties",
  );
  assert(!/transition:\s*all/.test(rule), "interaction must not transition all properties");
  assert(/scale\(1\.015\)/.test(globalsCss), "hover scale must stay bounded");
  assert(/scale\(0\.975\)/.test(globalsCss), "press scale must stay bounded");
  assert(/\.ct-liquid-interactive:not\(:disabled\):active\s*\{[^}]*--ct-duration-liquid-press/s.test(globalsCss), "press must use the liquid press token");
  assert(/\.ct-liquid-interactive:not\(:disabled\):active/.test(globalsCss), "disabled controls must not receive a press transform");
  assert(
    /\.ct-liquid-interactive:focus-visible,[^}]*\{[^}]*outline:\s*2px solid var\(--ct-accent\)[^}]*box-shadow:/s.test(globalsCss),
    "visible focus outline must be preserved",
  );
  const reduced = globalsCss.match(/@media \(prefers-reduced-motion: reduce\)\s*\{[^]*?\.ct-liquid-interactive[^]*?\n\}/)?.[0] ?? "";
  assert(/transition:\s*none/.test(reduced), "reduced motion must remove transitions");
  assert(/animation:\s*none/.test(reduced), "reduced motion must remove animations");
  assert(/transform:\s*var\(--ct-liquid-base-transform\)/.test(reduced), "reduced motion must neutralize scale");
  assert(!/background(?:-color)?:|color:/.test(reduced), "reduced motion must preserve visible state colors");
  assert(!/outline:\s*none/.test(reduced), "reduced motion must preserve the focus ring");
  assert(/useTouchOptimizedUi/.test(readFileSync(fileURLToPath(import.meta.url).replace(/\.test\.tsx$/, ".tsx"), "utf8")), "Button must use the shared touch-mode path");
});

test("Button md size inherits horizontal padding from ct-btn base", () => {
  assert(
    /\.ct-btn\s*\{[^}]*padding-left:\s*0\.75rem/s.test(globalsCss),
    "ct-btn base must define horizontal padding for default/md buttons",
  );
});

test("Button ghost has ct-btn-ghost class", async () => {
  const { host } = await render(<Button variant="ghost">Cancel</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-ghost"), `expected ct-btn-ghost in ${btn.className}`);
});

test("Button accent has ct-btn-accent class", async () => {
  const { host } = await render(<Button variant="accent">Action</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-accent"), `expected ct-btn-accent in ${btn.className}`);
});

test("Button danger has ct-btn-danger class", async () => {
  const { host } = await render(<Button variant="danger">Delete</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-danger"), `expected ct-btn-danger in ${btn.className}`);
});

test("Button size lg has ct-btn-lg class", async () => {
  const { host } = await render(<Button variant="primary" size="lg">Big</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-lg"), `expected ct-btn-lg in ${btn.className}`);
});

test("Button disabled is not interactive", async () => {
  const { host } = await render(<Button variant="primary" disabled>Nope</Button>);
  const btn = host.querySelector("button")!;
  assert((btn as HTMLButtonElement).disabled, "button must be disabled");
});

test("Button loading is disabled and shows BusyState", async () => {
  const { host } = await render(<Button variant="primary" loading>Working</Button>);
  const btn = host.querySelector("button")!;
  assert((btn as HTMLButtonElement).disabled, "loading button must be disabled");
  assert(btn.getAttribute("aria-busy") === "true", "aria-busy=true when loading");
  const dots = host.querySelectorAll(".ct-phase-dots");
  assert(dots.length > 0, "BusyState dots must render in loading state");
  assert(btn.textContent?.includes("Working"), "loading label must remain visible");
});

test("Button data-testid propagates", async () => {
  const { host } = await render(<Button data-testid="my-btn">OK</Button>);
  assert(host.querySelector('[data-testid="my-btn"]'), "testid not found");
});

test("Button click fires handler", async () => {
  let clicked = false;
  const { host } = await render(<Button onClick={() => { clicked = true; }}>Click</Button>);
  await act(async () => {
    (host.querySelector("button") as HTMLButtonElement).click();
  });
  assert(clicked, "onClick not fired");
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
    console.error(`\n✗ ${failed}/${tests.length} Button tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} Button tests green`);
}

void main();
