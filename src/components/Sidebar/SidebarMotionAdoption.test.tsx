// Run: npx tsx src/components/Sidebar/SidebarMotionAdoption.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { TeamTaskBucket } from "./TeamUnit";
import { useTeamPlanMotion } from "./TeamUnit";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
  Node: window.Node,
  Event: window.Event,
  AnimationEvent: window.AnimationEvent,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const colors = ["blue", "yellow", "red", "green"];
const keys = ["todo", "review", "blocked", "done"] as const;
const counts = (values: number[]): TeamTaskBucket[] => keys.map((key, index) => ({
  key,
  label: key,
  count: values[index],
  color: colors[index],
}));

function Harness({ values }: { values: number[] }) {
  const motion = useTeamPlanMotion(counts(values));
  return (
    <div
      data-revision={motion.revision}
      data-reacting={motion.reacting ? "true" : "false"}
      data-sweep={motion.completionSweep ? "true" : "false"}
      onAnimationEnd={motion.onAnimationEnd}
    />
  );
}

async function renderSequence(sequence: number[][]) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const snapshots: Array<{ revision: string | null; sweep: string | null }> = [];
  for (const values of sequence) {
    await act(async () => root.render(<Harness values={values} />));
    const node = host.firstElementChild!;
    snapshots.push({
      revision: node.getAttribute("data-revision"),
      sweep: node.getAttribute("data-sweep"),
    });
  }
  return { host, root, snapshots };
}

async function run() {
  for (const initial of [[2, 0, 0, 1], [0, 0, 0, 3], [0, 0, 0, 0]]) {
    const { root, snapshots } = await renderSequence([initial]);
    assert.deepEqual(snapshots, [{ revision: "0", sweep: "false" }], "initial plans are static");
    await act(async () => root.unmount());
  }

  const changed = await renderSequence([
    [2, 0, 0, 1],
    [1, 0, 0, 2],
    [1, 0, 0, 2],
    [0, 0, 0, 3],
    [0, 0, 0, 3],
  ]);
  assert.deepEqual(changed.snapshots, [
    { revision: "0", sweep: "false" },
    { revision: "1", sweep: "false" },
    { revision: "1", sweep: "false" },
    { revision: "2", sweep: "true" },
    { revision: "2", sweep: "true" },
  ], "bucket changes arm once and only an incomplete-to-complete change arms the sweep");
  const reactionEvent = new Event("animationend", { bubbles: true });
  Object.defineProperty(reactionEvent, "animationName", { value: "sidebar-team-donut-grow" });
  await act(async () => changed.host.firstElementChild!.dispatchEvent(reactionEvent));
  assert.equal(changed.host.firstElementChild!.getAttribute("data-reacting"), "false", "reaction class clears from its animation event");
  const sweepEvent = new Event("animationend", { bubbles: true });
  Object.defineProperty(sweepEvent, "animationName", { value: "sidebar-team-completion-sweep" });
  await act(async () => changed.host.firstElementChild!.dispatchEvent(sweepEvent));
  assert.equal(changed.host.firstElementChild!.getAttribute("data-sweep"), "false", "sweep clears from its animation event");
  await act(async () => changed.root.unmount());

  const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  const sources = [
    readFileSync(new URL("./TeamUnit.tsx", import.meta.url), "utf8"),
    readFileSync(new URL("./WorkspaceCardGrid.tsx", import.meta.url), "utf8"),
  ].join("\n");
  assert.doesNotMatch(sources, /transition-all|transition:\s*all|setTimeout/);
  assert.match(styles, /\.sidebar-team-card-activation[^}]*transform-origin:\s*center/s);
  assert.match(styles, /\.sidebar-pane-activation[^}]*transform-origin:\s*center/s);
  assert.match(styles, /\.sidebar-workspace-card-select[^}]*transform-origin:\s*center/s);
  assert.match(styles, /\.sidebar-workspace-card-create[^}]*transform-origin:\s*center/s);
  assert.match(styles, /scale\(1\.02\)/);
  assert.match(styles, /scale\(\.955\)/);
  assert.match(styles, /@keyframes sidebar-team-donut-grow\{0%\{transform:scale\(\.955\)\}58%\{transform:scale\(1\.02\)\}100%\{transform:scale\(1\)\}\}/);
  assert.match(styles, /@keyframes sidebar-team-count-pop\{0%\{transform:scale\(\.955\)\}55%\{transform:scale\(1\.02\)\}100%\{transform:scale\(1\)\}\}/);
  assert.match(styles, /@keyframes sidebar-team-completion-sweep\{to\{transform:translateX\(120%\)\}\}/);
  assert.match(styles, /@container \(max-width: 159px\)[\s\S]*?\.sidebar-team-readout-separator\s*\{\s*display:\s*none;/);
  assert.match(styles, /\.sidebar-team-card-activation:focus-visible[\s\S]*?outline:\s*1px solid var\(--ct-accent\)/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.sidebar-team-donut-reaction[\s\S]*?animation:\s*none/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.sidebar-team-card-activation[\s\S]*?transform:\s*none/);
  assert.match(styles, /\.sidebar-team-header-active:hover[\s\S]*?background:\s*var\(--ct-hover\)/);
  assert.match(styles, /\.sidebar-workspace-card\[data-selected="true"\][\s\S]*?background:\s*var\(--ct-accent\)/);
  assert.match(styles, /\.sidebar-workspace-card-working[\s\S]*?background:\s*color-mix/);
}

run().then(
  () => console.log("Sidebar motion adoption tests passed"),
  (error) => {
    console.error(error);
    process.exitCode = 1;
  },
);
