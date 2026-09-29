import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LiquidTabRail } from "./LiquidTabRail";

const window = new Window({ url: "http://localhost/" });
Object.assign(globalThis, {
  window,
  document: window.document,
  HTMLElement: window.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: true,
});
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
let selected = "dreams";

async function main() {
  await act(async () => {
    root.render(
      <LiquidTabRail
        value={selected}
        options={["dreams", "inspect", "activity", "settings"]}
        onChange={(next) => {
          selected = next;
        }}
        label="Memory sections"
      />,
    );
  });

  const settings = host.querySelector<HTMLButtonElement>('[role="tab"]:last-of-type');
  assert(settings);
  await act(async () => settings.click());
  assert.equal(selected, "settings");
  const indicator = host.querySelector<HTMLElement>(".ct-liquid-tab-rail__indicator--travel");
  assert.equal(indicator?.style.getPropertyValue("--sx-from"), "0%");
  assert.equal(indicator?.style.getPropertyValue("--sx-to"), "300%");
  assert.equal(host.querySelector('[role="tablist"]')?.getAttribute("aria-label"), "Memory sections");
  const firstTab = host.querySelector<HTMLButtonElement>('[role="tab"]');
  assert(firstTab);
  await act(async () => {
    firstTab.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }) as unknown as Event);
  });
  assert.equal(selected, "inspect");
  assert.equal(document.activeElement, host.querySelector('[role="tab"]:nth-of-type(2)'));
  const css = readFileSync(resolve(import.meta.dirname, "../../styles/globals.css"), "utf8");
  // Anchor on the block, not the bare query: the shell breakpoint variants mention the same text.
  const mobileStart = css.indexOf("\n@media (max-width: 640px) {");
  const mobile = css.slice(mobileStart, css.indexOf(".ct-liquid-toggle", mobileStart));
  assert.match(mobile, /\.ct-section-toolbar__tabs[\s\S]*flex:\s*0 0 100%/);
  assert.match(mobile, /\.ct-liquid-tab-rail[\s\S]*min-width:\s*0/);
  assert.match(css, /\.ct-liquid-tab-rail\s*\{[^}]*overflow:\s*hidden/s);
  assert.match(css, /\.ct-liquid-tab-rail__indicator\s*\{[^}]*bottom:\s*3px[^}]*height:\s*2px/s);
  assert.match(css, /\.ct-liquid-tab-rail__tab\s*\{[^}]*font-size:\s*0\.75rem/s);

  await act(async () => root.unmount());
  console.log("liquid tab rail tests: passed");
}

void main();
