// Run: npx tsx src/components/shared/PlanStatusBreakdown.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { PlanStatusBreakdown } from "./PlanStatusBreakdown";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
  Node: window.Node,
  Event: window.Event,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(
    <PlanStatusBreakdown
      data-testid="breakdown"
      counts={{ todo: 4, review: 3, blocked: 2, paused: 1, done: 1 }}
    />,
  ));

  const breakdown = host.querySelector('[data-testid="breakdown"]');
  assert(breakdown, "breakdown renders");
  for (const [status, count] of Object.entries({
    todo: 4,
    review: 3,
    blocked: 2,
    paused: 1,
    done: 1,
  })) {
    const item = host.querySelector<HTMLElement>(`[data-status="${status}"]`);
    assert(item, `${status} renders`);
    assert.equal(item.textContent, `${count} ${status}`);
    assert.equal(
      item.style.color,
      `var(--ct-${status === "todo"
        ? "accent"
        : status === "review"
          ? "yellow"
          : status === "blocked"
            ? "red"
            : status === "paused"
              ? "muted"
              : "green"})`,
    );
  }
  const separators = host.querySelectorAll('[data-testid="plan-status-separator"]');
  assert.equal(separators.length, 4, "five status counts have four real separators");
  for (const separator of separators) assert.equal(separator.textContent?.trim(), "·");
  assert.equal(
    breakdown.getAttribute("aria-label"),
    "4 todo, 3 review, 2 blocked, 1 paused, 1 done",
    "the one-line visual has a concise accessible summary",
  );
  await act(async () => root.unmount());

  const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  assert.match(
    styles,
    /\.ct-plan-status-breakdown\s*\{[^}]*display:\s*(?:inline-)?flex;[^}]*flex-wrap:\s*nowrap;[^}]*white-space:\s*nowrap;/s,
    "breakdown is a single non-wrapping line",
  );
  assert.match(
    styles,
    /\.ct-plan-status-breakdown-segment\s*\{[^}]*display:\s*inline-flex;[^}]*white-space:\s*nowrap;/s,
    "separator and following count cannot wrap apart",
  );

  console.log("PlanStatusBreakdown tests passed");
}

void main();
