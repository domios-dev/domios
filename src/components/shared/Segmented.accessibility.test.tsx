// Run: npx tsx src/components/shared/Segmented.accessibility.test.tsx

import { act } from "react";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import { Segmented } from "./Segmented";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(
      <Segmented
        value="chat"
        options={[
          { value: "chat", label: "Chat" },
          { value: "terminal", label: "Terminal" },
        ]}
        onChange={() => {}}
        data-testid="tab-mode-switch"
        data-tab-id="tab-a"
        aria-label="Tab view"
      />,
    );
  });

  const switcher = host.querySelector('[data-testid="tab-mode-switch"]');
  if (switcher?.getAttribute("role") !== "tablist") throw new Error("switch is a tablist");
  if (switcher?.getAttribute("aria-label") !== "Tab view") throw new Error("switch is named");
  if (switcher?.getAttribute("data-tab-id") !== "tab-a") throw new Error("switch exposes Tab scope");
  const chat = host.querySelector('[role="tab"][data-value="chat"]');
  const terminal = host.querySelector('[role="tab"][data-value="terminal"]');
  if (chat?.getAttribute("aria-selected") !== "true") throw new Error("active view is exposed");
  if (terminal?.getAttribute("aria-selected") !== "false") throw new Error("inactive view is exposed");
  if (chat?.tagName !== "BUTTON" || terminal?.tagName !== "BUTTON") throw new Error("segments are keyboard-operable buttons");
  await act(async () => {
    (terminal as HTMLButtonElement).click();
  });
  const thumb = host.querySelector<HTMLElement>(".ct-liquid-segmented__thumb--travel");
  assert.equal(thumb?.style.getPropertyValue("--sx-from"), "0%");
  assert.equal(thumb?.style.getPropertyValue("--sx-to"), "100%");

  const segmentedSource = readFileSync(resolve(import.meta.dirname, "Segmented.tsx"), "utf8");
  assert.match(segmentedSource, /"--sx-from"/);
  assert.match(segmentedSource, /"--sx-to"/);
  assert.match(segmentedSource, /ct-liquid-segmented__thumb--travel/);
  assert.match(segmentedSource, /onAnimationEnd/);
  assert.doesNotMatch(segmentedSource, /text-\[var\(--ct-bg\)\]/);
  assert.doesNotMatch(segmentedSource, /duration-200|ease-out/);

  const globals = ["globals.css", "primitives.css"]
    .map((name) => readFileSync(resolve(import.meta.dirname, "../../styles", name), "utf8"))
    .join("\n");
  const tokens = readFileSync(resolve(import.meta.dirname, "../../styles/tokens.css"), "utf8");
  assert.match(globals, /\.ct-liquid-segmented__option\[aria-selected="true"\]\s*\{[^}]*color:\s*var\(--ct-fg\)/s);
  assert.doesNotMatch(globals, /\.ct-liquid-segmented__option\[aria-selected="true"\]\s*\{[^}]*var\(--ct-bg\)/s);
  assert.match(globals, /\.ct-liquid-segmented__option:focus-visible/);
  const travelKeyframes = globals.match(/@keyframes ct-liquid-control-x\s*\{[\s\S]*?\n\}/)?.[0] ?? "";
  for (const stop of ["0%", "38%", "60%", "78%", "100%"]) assert.match(travelKeyframes, new RegExp(stop.replace("%", "%")));
  assert.match(travelKeyframes, /scale\(1\.3,\s*0\.82\)/);
  assert.match(travelKeyframes, /scale\(0\.86,\s*1\.09\)/);
  assert.match(tokens, /--ct-duration-liquid-segmented:\s*420ms/);
  assert.match(tokens, /--ct-duration-liquid-toggle:\s*380ms/);
  assert.match(tokens, /--ct-duration-liquid-flip:\s*360ms/);
  assert.match(globals, /ct-liquid-control-x var\(--ct-duration-liquid-segmented\) both/);
  assert.match(globals, /ct-liquid-control-knob var\(--ct-duration-liquid-toggle\) both/);
  assert.match(globals, /ct-liquid-control-x var\(--ct-duration-liquid-flip\) both/);
  assert.doesNotMatch(globals, /--ct-duration-liquid-dots/);
  const reducedMotionBlocks = globals.match(/@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\n\}/g) ?? [];
  assert(reducedMotionBlocks.some((block) => /\.ct-liquid-segmented__thumb/.test(block) && /animation: none !important/.test(block)));

  await act(async () => root.unmount());
  console.log("ok - liquid segmented accessibility and motion contract");
}

void main();
