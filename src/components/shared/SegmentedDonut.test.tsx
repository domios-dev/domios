// Run: npx tsx src/components/shared/SegmentedDonut.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  PLAN_STATUS_COLORS,
  SegmentedDonut,
  segmentedDonutBackground,
  type SegmentedDonutSegment,
} from "./SegmentedDonut";

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

const segments = (
  todo: number,
  review: number,
  blocked: number,
  done: number,
): SegmentedDonutSegment[] => [
  { status: "todo", count: todo },
  { status: "review", count: review },
  { status: "blocked", count: blocked },
  { status: "done", count: done },
];

async function main() {
  assert.deepEqual(PLAN_STATUS_COLORS, {
    todo: "var(--ct-accent)",
    review: "var(--ct-yellow)",
    blocked: "var(--ct-red)",
    done: "var(--ct-green)",
  });
  assert.equal(
    segmentedDonutBackground(segments(2, 1, 1, 4)),
    "conic-gradient(var(--ct-accent) 0% 25%, var(--ct-yellow) 25% 37.5%, var(--ct-red) 37.5% 50%, var(--ct-green) 50% 100%)",
    "mixed segments are contiguous and preserve status order",
  );
  assert.equal(
    segmentedDonutBackground(segments(0, 0, 3, 0)),
    "conic-gradient(var(--ct-red) 0% 100%)",
    "blocked-only input fills the ring",
  );
  assert.equal(
    segmentedDonutBackground(segments(0, 0, 0, 5)),
    "conic-gradient(var(--ct-green) 0% 100%)",
    "all-done input fills the ring",
  );
  assert.equal(
    segmentedDonutBackground(segments(0, 0, 0, 0)),
    "color-mix(in srgb, var(--ct-fg) 14%, transparent)",
    "empty input uses a theme-derived neutral ring",
  );
  assert.equal(
    segmentedDonutBackground([{ count: 1, color: "var(--ct-purple)" }]),
    "conic-gradient(var(--ct-purple) 0% 100%)",
    "callers may provide an explicit runtime-theme colour",
  );

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(
    <SegmentedDonut
      data-testid="donut"
      segments={segments(1, 1, 0, 2)}
      size={22}
      aria-label="Task status composition"
    />,
  ));
  const donut = host.querySelector<HTMLElement>('[data-testid="donut"]');
  assert(donut, "donut renders");
  assert.equal(donut.style.getPropertyValue("--ct-segmented-donut-size"), "22px");
  assert.equal(
    donut.style.getPropertyValue("--ct-segmented-donut-background"),
    "conic-gradient(var(--ct-accent) 0% 25%, var(--ct-yellow) 25% 50%, var(--ct-green) 50% 100%)",
  );
  assert(host.querySelector('[data-testid="segmented-donut-hole"]'), "donut renders its hole");
  await act(async () => root.unmount());

  const componentSource = readFileSync(new URL("./SegmentedDonut.tsx", import.meta.url), "utf8");
  const teamSource = readFileSync(new URL("../Sidebar/TeamUnit.tsx", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  assert.doesNotMatch(componentSource, /#[0-9a-f]{3,8}\b/i, "shared donut has no hardcoded hex");
  assert.doesNotMatch(teamSource, /conic-gradient/, "TeamUnit contains no second donut implementation");
  assert.match(
    styles,
    /\.ct-segmented-donut[^}]*animation:[^;]*var\(--ct-duration-liquid-donut\)/s,
    "segment growth uses the liquid donut duration",
  );
  assert.match(
    styles,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.ct-segmented-donut[^}]*animation:\s*none/s,
    "reduced motion renders without growth animation",
  );
  assert.match(
    styles,
    /\.sidebar-plan-entry-completion::after\s*\{[^}]*animation:\s*sidebar-plan-entry-completion-sweep var\(--ct-duration-liquid-sweep\)[^;}]*;/s,
    "the completion sweep is scoped to the plan entry and uses its duration token",
  );
  assert.match(
    styles,
    /\.sidebar-plan-entry-count-completion[^}]*\.ct-plan-status-breakdown-count\s*\{[^}]*animation:\s*sidebar-plan-entry-count-pop/s,
    "the plan-entry done counter pops with the completion sweep",
  );
  assert.doesNotMatch(styles, /sidebar-plan-entry-completion-sweep[^}]*\binfinite\b/s);

  console.log("SegmentedDonut tests passed");
}

void main();
