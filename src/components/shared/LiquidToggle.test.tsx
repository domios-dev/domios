import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { LiquidFlip, LiquidToggle } from "./LiquidToggle";

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
  const knob = host.querySelector<HTMLElement>(".ct-liquid-toggle__knob--travel");
  assert(knob, "toggle starts gallery travel lifecycle");
  assert.equal(knob.style.getPropertyValue("--kx-from"), "0px");
  assert.equal(knob.style.getPropertyValue("--kx-to"), "var(--ct-liquid-toggle-travel)");

  let meshEnabled = false;
  await act(async () => root.render(
    <LiquidFlip
      aria-label="Mesh presence"
      checked={meshEnabled}
      onCheckedChange={(next) => { meshEnabled = next; }}
    />,
  ));
  const flip = host.querySelector('button[role="switch"]') as HTMLButtonElement;
  assert.equal(flip.textContent, "offon");
  assert.equal(flip.getAttribute("aria-checked"), "false");
  await act(async () => flip.click());
  assert.equal(meshEnabled, true, "flip invokes checked callback");
  const chip = host.querySelector<HTMLElement>(".ct-liquid-flip__chip--travel");
  assert(chip, "flip starts gallery travel lifecycle");
  assert.equal(chip.style.getPropertyValue("--sx-from"), "0%");
  assert.equal(chip.style.getPropertyValue("--sx-to"), "100%");

  const source = await import("node:fs").then(({ readFileSync }) =>
    readFileSync(new URL("./LiquidToggle.tsx", import.meta.url), "utf8"),
  );
  assert.match(source, /"--kx-from"/);
  assert.match(source, /"--kx-to"/);
  assert.match(source, /onAnimationEnd/);
  await act(async () => root.unmount());
  console.log("liquid toggle and flip tests: passed");
}
void run();
