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
const THEME_PAINT = /^(none|currentColor|var\(--ct-[a-z-]+\)|color-mix\(in srgb, var\(--ct-[a-z-]+\) \d+%, (var\(--ct-[a-z-]+\)|transparent)\))$/;

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

  const scrolled: string[] = [];
  const proto = window.HTMLElement.prototype as unknown as { scrollIntoView: () => void };
  const original = proto.scrollIntoView;
  proto.scrollIntoView = function (this: HTMLElement) {
    scrolled.push(this.getAttribute("data-settings-card") ?? "");
  };
  try {
    const { host: cardHost } = await render(
      <SettingsCardAnchor id={route.card} focused>
        <span>card</span>
      </SettingsCardAnchor>,
    );
    const anchor = cardHost.querySelector(`[data-settings-card="${route.card}"]`);
    assert(!!anchor, "card anchor did not render");
    assert(JSON.stringify(scrolled) === JSON.stringify([route.card]), `expected one scroll to the card, got ${JSON.stringify(scrolled)}`);
    assert(document.activeElement === anchor, "card anchor did not take focus");
  } finally {
    proto.scrollIntoView = original;
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
    for (const el of [svg, ...Array.from(svg.querySelectorAll("*"))]) {
      for (const attr of ["fill", "stroke"]) {
        const paint = el.getAttribute(attr);
        if (paint !== null) assert(THEME_PAINT.test(paint), `${id} uses a non-theme ${attr}: ${paint}`);
      }
    }
  }
  assert(new Set(labels).size === labels.length, "figures share one aria-label");
  assert(!section.querySelector('[data-testid="help-figure"] img'), "figures must not use raster images");

  const markerIds = Array.from(host.querySelectorAll("marker")).map((m) => m.getAttribute("id"));
  assert(new Set(markerIds).size === markerIds.length, `duplicate marker ids ${JSON.stringify(markerIds)}`);
});

test("help_supporter_section_puts_plain_language_before_folded_details", async () => {
  const { host } = await render(
    <HelpPanel config={config} initialSection="supporter-agent" onOpenSettings={() => undefined} />,
  );
  const section = supporterSection(host);
  const folds = Array.from(section.querySelectorAll('details[data-testid="help-details"]'));
  assert(folds.length >= 2, `expected technical and model folds, got ${folds.length}`);
  for (const fold of folds) {
    assert(!fold.hasAttribute("open"), "technical details must start folded");
    assert(fold.querySelector("summary")?.textContent?.trim(), "fold has no summary");
  }
  const firstFold = folds[0];
  for (const selector of ['[data-testid="help-figure"]', '[data-testid="help-settings-link"]', '[data-testid="help-paragraph"]']) {
    const el = section.querySelector(selector);
    assert(!!el && !firstFold.contains(el), `${selector} should appear outside the folds`);
    assert(el.compareDocumentPosition(firstFold) & 4, `${selector} should come before the first fold`);
  }
  assert(
    folds.some((fold) => fold.querySelector('[data-testid="help-measurement"]')),
    "decision model numbers placeholder missing from the folds",
  );
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
