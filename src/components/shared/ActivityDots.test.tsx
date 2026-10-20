// Run: npx tsx src/components/shared/ActivityDots.test.tsx

import assert from "node:assert/strict";
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
  assert.equal(dots.children.length, 3, "small dots render three dots");
  await act(async () => root.render(<ActivityDots size="micro" data-testid="micro" />));
  assert.equal(host.querySelector('[data-testid="micro"]')?.children.length, 1, "micro renders one breathing dot");

  await act(async () => root.render(<ActivityDots size="micro" variant="workspace-badge" data-testid="badge" />));
  assert.equal(host.querySelector('[data-testid="badge"]')?.children.length, 2, "workspace badge variant renders exactly two dots");

  await act(async () => root.unmount());
  console.log("activity dots tests: passed");
}

void run();
