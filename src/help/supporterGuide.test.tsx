// Run: npx tsx src/help/supporterGuide.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { HelpPanel } from "./HelpPanel";
import type { HelpSettingsTarget } from "./types";
import type { AppConfig } from "../lib/types";
import { SETTINGS_SECTIONS, settingsRouteForHelpTarget } from "../components/Settings/sections";
import { SettingsCardAnchor } from "../components/Settings/SettingsCardAnchor";

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
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLButtonElement",
    "HTMLSelectElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ] as const) {
    const value = key === "window" ? window : (window as unknown as Record<string, unknown>)[key];
    Object.defineProperty(globalThis, key, { value, configurable: true });
  }
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

const config = { keybindings: {} } as AppConfig;

function supporterSection(host: HTMLElement): Element {
  const section = host.querySelector('[data-help-section-id="supporter-agent"]');
  assert(!!section, "supporter agent section did not render");
  return section;
}

test("help_settings_link_opens_supporter_agent_card", async () => {
  const opened: HelpSettingsTarget[] = [];
  const { host, root } = await render(
    <HelpPanel config={config} initialSection="supporter-agent" onOpenSettings={(t) => opened.push(t)} />,
  );
  const links = supporterSection(host).querySelectorAll('[data-testid="help-settings-link"]');
  assert(links.length === 1, `expected one settings link, got ${links.length}`);
  const link = links[0] as HTMLButtonElement;
  assert(link.tagName === "BUTTON" && link.getAttribute("type") === "button", "settings link must be a button");
  const name = (link.getAttribute("aria-label") ?? link.textContent ?? "").trim();
  assert(name.length > 0, "settings link has no accessible name");
  assert(link.querySelector("svg")?.closest('[aria-hidden="true"]'), "decorative icon must be hidden from assistive tech");

  await act(async () => link.click());
  assert(JSON.stringify(opened) === JSON.stringify(["supporter-agent"]), `unexpected targets ${JSON.stringify(opened)}`);

  const route = settingsRouteForHelpTarget(opened[0]);
  assert(route.section === "agents" && route.card === "supporter-agent", `wrong route ${JSON.stringify(route)}`);
  assert(SETTINGS_SECTIONS.some((s) => s.id === route.section), "route section is not a settings section");
  await act(async () => root.unmount());

  // The card lands at the top of its own scroller and never scrolls an overflow-hidden ancestor.
  const proto = window.HTMLElement.prototype as unknown as { getBoundingClientRect: () => DOMRect };
  const original = proto.getBoundingClientRect;
  proto.getBoundingClientRect = function (this: HTMLElement) {
    return { top: this.hasAttribute("data-settings-card") ? 300 : 0 } as DOMRect;
  };
  try {
    const { host: cardHost } = await render(
      <div data-testid="outer" style={{ overflow: "hidden" }}>
        <div data-testid="scroller" style={{ overflowY: "auto" }}>
          <SettingsCardAnchor id={route.card} focused>
            <span>card</span>
          </SettingsCardAnchor>
        </div>
      </div>,
    );
    const anchor = cardHost.querySelector(`[data-settings-card="${route.card}"]`);
    assert(!!anchor, "card anchor did not render");
    const scroller = cardHost.querySelector<HTMLElement>('[data-testid="scroller"]')!;
    const outer = cardHost.querySelector<HTMLElement>('[data-testid="outer"]')!;
    assert(scroller.scrollTop === 284, `expected the scroller at the card, got ${scroller.scrollTop}`);
    assert(outer.scrollTop === 0, "an overflow-hidden ancestor scrolled");
    assert(document.activeElement === anchor, "card anchor did not take focus");
  } finally {
    proto.getBoundingClientRect = original;
  }
});

test("help_settings_link_hidden_without_a_settings_route", async () => {
  const { host } = await render(<HelpPanel config={config} initialSection="supporter-agent" />);
  assert(!supporterSection(host).querySelector('[data-testid="help-settings-link"]'), "link rendered with nowhere to go");
});

test("help_supporter_figures_have_role_img_and_aria_label", async () => {
  const { host } = await render(<HelpPanel config={config} initialSection="supporter-agent" />);
  const section = supporterSection(host);
  const labels: string[] = [];
  for (const id of ["supporter-recall", "supporter-tab-state"]) {
    const svg = section.querySelector(`[data-help-figure="${id}"] svg`);
    assert(!!svg, `${id} figure missing`);
    assert(svg.getAttribute("role") === "img", `${id} lacks role=img`);
    const label = svg.getAttribute("aria-label") ?? "";
    assert(label.trim().length > 20, `${id} lacks a descriptive aria-label`);
    labels.push(label);
  }
  assert(new Set(labels).size === labels.length, "figures share one aria-label");
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
  console.log(`help/supporterGuide tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
