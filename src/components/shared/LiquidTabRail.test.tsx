import assert from "node:assert/strict";
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
  assert.equal(host.querySelector('[role="tablist"]')?.getAttribute("aria-label"), "Memory sections");
  const firstTab = host.querySelector<HTMLButtonElement>('[role="tab"]');
  assert(firstTab);
  await act(async () => {
    firstTab.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }) as unknown as Event);
  });
  assert.equal(selected, "inspect");
  assert.equal(document.activeElement, host.querySelector('[role="tab"]:nth-of-type(2)'));

  await act(async () => root.unmount());
  console.log("liquid tab rail tests: passed");
}

void main();
