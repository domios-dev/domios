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
    assert(nav?.getAttribute("aria-current") === "location", "supporter agent section was not selected");
    assert(JSON.stringify(scrolled) === JSON.stringify(["supporter-agent"]), `expected one scroll to the section, got ${JSON.stringify(scrolled)}`);
  } finally {
    proto.scrollIntoView = original;
  }
});

test("nav selects plans section", async () => {
  const { host } = await render(<HelpPanel config={config} />);
  const plansNav = host.querySelector('[data-help-nav="plans"]') as HTMLButtonElement | null;
  assert(!!plansNav, "plans nav item missing");
  await act(async () => {
    plansNav!.click();
  });
  const active = host.querySelector('[data-help-nav="plans"]');
  assert(active?.getAttribute("aria-current") === "location", "plans nav should highlight after click");
});

test("content scroll updates the current navigation entry and section selector", async () => {
  const { host, root } = await render(<HelpPanel config={config} />);
  try {
    const scrollport = host.querySelector('[data-testid="help-content-tab"]') as HTMLDivElement;
    Object.defineProperty(scrollport, "clientHeight", { value: 600 });
    Object.defineProperty(scrollport, "scrollHeight", { value: 20000 });
    scrollport.getBoundingClientRect = () => ({ top: 100, bottom: 700 } as DOMRect);
    const sections = Array.from(host.querySelectorAll<HTMLElement>("[data-help-section]"));
    let offset = 0;
    sections.forEach((section, index) => {
      section.getBoundingClientRect = () => ({
        top: 100 + index * 1000 - offset,
        bottom: 1060 + index * 1000 - offset,
      } as DOMRect);
    });
    for (const index of [0, 1, 2, 1, 0]) {
      offset = index * 1000;
      await act(async () => {
        scrollport.scrollTop = offset;
        scrollport.dispatchEvent(new window.Event("scroll"));
      });
      const id = sections[index].getAttribute("data-help-section");
      const current = host.querySelectorAll('[data-help-nav][aria-current="location"]');
      assert(current.length === 1 && current[0].getAttribute("data-help-nav") === id,
        `scroll should select ${id}`);
      const select = host.querySelector("select") as HTMLSelectElement;
      assert(select.value === id, "narrow section selector should follow the same scroll state");
    }
    await act(async () => { root.unmount(); });
    await act(async () => { scrollport.dispatchEvent(new window.Event("scroll")); });
  } finally {
    host.remove();
  }
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
