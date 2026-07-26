// Run: npx tsx src/components/shared/ActivityDots.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ActivityDots } from "./ActivityDots";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

async function run() {
  await act(async () => root.render(
    <ActivityDots size="small" tone="thinking" rhythm="streaming" data-testid="dots" />,
  ));
  const dots = host.querySelector('[data-testid="dots"]');
  assert(dots, "ActivityDots renders");
  assert.equal(dots.getAttribute("aria-hidden"), "true");
  assert.equal(dots.children.length, 3, "ActivityDots always renders three dots");
  assert(dots.classList.contains("activity-dots--small"));
  assert(dots.classList.contains("activity-dots--thinking"));
  assert(dots.classList.contains("activity-dots--streaming"));

  await act(async () => root.render(<ActivityDots idle />));
  assert(host.querySelector(".activity-dots--idle"), "idle state is static");

  const css = readFileSync(new URL("../../styles/agent-state.css", import.meta.url), "utf8");
  assert.match(css, /\.activity-dots--micro\s*\{[^}]*--activity-dot-size:\s*2\.5px;[^}]*--activity-dot-gap:\s*1\.25px;/s);
  assert.match(css, /\.activity-dots--small\s*\{[^}]*--activity-dot-size:\s*3px;[^}]*--activity-dot-gap:\s*2px;/s);
  assert.match(css, /\.activity-dots--medium\s*\{[^}]*--activity-dot-size:\s*5px;[^}]*--activity-dot-gap:\s*3px;/s);
  const reducedMotion = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));
  assert.match(reducedMotion, /\.activity-dots i\s*\{[^}]*animation:\s*none !important;[^}]*opacity:\s*0\.85;/s);
  assert(!/\.activity-dots--micro i:nth-child\([23]\)\s*\{[^}]*display:\s*none/s.test(css), "micro keeps all three dots visible");
  assert.equal((css.match(/@keyframes ct-dots-squash-jump/g) ?? []).length, 1, "one shared dot keyframe family");

  await act(async () => root.unmount());
  console.log("activity dots tests: passed");
}

void run();
