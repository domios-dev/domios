import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LiquidToggle } from "./LiquidToggle";

const window = new Window({ url: "http://localhost/" });
Object.assign(globalThis, { window, document: window.document, HTMLElement: window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
let checked = false;

async function run() {
  await act(async () => root.render(<LiquidToggle aria-label="Enable feature" checked={checked} onCheckedChange={(next) => { checked = next; }} />));
  const toggle = host.querySelector('button[role="switch"]') as HTMLButtonElement;
  assert(toggle && toggle.getAttribute("aria-checked") === "false");
  await act(async () => toggle.click());
  assert.equal(checked, true, "switch invokes checked callback");
  assert(toggle.classList.contains("ct-liquid-toggle"));
  await act(async () => root.unmount());
  console.log("liquid toggle tests: passed");
}
void run();
