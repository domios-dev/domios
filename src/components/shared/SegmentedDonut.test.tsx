// Run: npx tsx src/components/shared/SegmentedDonut.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
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
  assert(host.querySelector('[data-testid="segmented-donut-hole"]'), "donut renders its hole");
  await act(async () => root.unmount());

  const teamSource = readFileSync(new URL("../Sidebar/TeamUnit.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(teamSource, /conic-gradient/, "TeamUnit contains no second donut implementation");

  console.log("SegmentedDonut tests passed");
}

void main();
