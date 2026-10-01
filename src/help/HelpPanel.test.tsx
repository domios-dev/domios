// Run: npx tsx src/help/HelpPanel.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { HelpPanel } from "./HelpPanel";
import type { AppConfig } from "../lib/types";

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
  Object.defineProperty(globalThis, "HTMLSelectElement", { value: window.HTMLSelectElement, configurable: true });
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

const config = {
  keybindings: { reopen_tab: "Ctrl+Shift+T" },
} as AppConfig;

test("renders guide sections from content module", async () => {
  const { host } = await render(<HelpPanel config={config} />);
  const sections = host.querySelectorAll('[data-testid="help-section"]');
  assert(sections.length === 9, `expected 9 sections, got ${sections.length}`);
  const overview = host.querySelector('[data-help-section-id="overview"]');
  assert(overview?.textContent?.includes("What is CodeTerm"), "overview copy missing");
});

test("initialSection renders, selects and scrolls to the supporter agent section", async () => {
  const scrolled: string[] = [];
  const proto = window.HTMLElement.prototype as unknown as { scrollIntoView: () => void };
  const original = proto.scrollIntoView;
  proto.scrollIntoView = function (this: HTMLElement) {
    scrolled.push(this.getAttribute("data-help-section") ?? "");
  };
  try {
    const { host } = await render(<HelpPanel config={config} initialSection="supporter-agent" />);
    const section = host.querySelector('[data-help-section-id="supporter-agent"]');
    const nav = host.querySelector('[data-help-nav="supporter-agent"]');
    assert(!!section, "supporter agent section did not render");
    assert(nav?.className.includes("accent"), "supporter agent section was not selected");
    assert(JSON.stringify(scrolled) === JSON.stringify(["supporter-agent"]), `expected one scroll to the section, got ${JSON.stringify(scrolled)}`);
  } finally {
    proto.scrollIntoView = original;
  }
});

test("renders inline SVG figures without raster images", async () => {
  const { host } = await render(<HelpPanel config={config} initialSection="agents" />);
  const figures = host.querySelectorAll('[data-testid="help-figure"] svg');
  assert(figures.length >= 3, "expected at least three inline SVG figures");
  assert(!host.querySelector('[data-testid="help-figure"] img'), "figures must not use raster images");
});

test("shortcut table pulls reopen_tab from config", async () => {
  const { host } = await render(<HelpPanel config={config} initialSection="tips" />);
  const row = host.querySelector('[data-help-shortcut="Reopen closed tab"]');
  assert(!!row, "reopen tab row missing");
  assert(row?.textContent?.includes("T"), "config binding not rendered");
});

test("nav selects plans section", async () => {
  const { host } = await render(<HelpPanel config={config} />);
  const plansNav = host.querySelector('[data-help-nav="plans"]') as HTMLButtonElement | null;
  assert(!!plansNav, "plans nav item missing");
  await act(async () => {
    plansNav!.click();
  });
  const active = host.querySelector('[data-help-nav="plans"]');
  assert(active?.className.includes("accent"), "plans nav should highlight after click");
});

setupDom();

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`help/HelpPanel tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
