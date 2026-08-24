// Run: npx tsx src/components/Settings/AgentNotificationLevels.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  AgentNotificationLevels,
  levelLabel,
  setLevelEnabled,
} from "./AgentNotificationLevels";
import type { AgentLevelChoice, AppConfig } from "../../lib/types";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLInputElement",
    "HTMLTextAreaElement",
    "HTMLSelectElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ]) {
    Object.defineProperty(globalThis, key, {
      value: window[key as keyof Window],
      configurable: true,
    });
  }
}

function configWith(agent_levels?: AgentLevelChoice[]): AppConfig {
  return { notifications: { agent_levels } } as unknown as AppConfig;
}

type UpdateCall = { section: string; patch: Record<string, unknown> };

async function render(
  config: AppConfig,
  updates: UpdateCall[],
): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <AgentNotificationLevels
        config={config}
        onUpdate={(section, patch) =>
          updates.push({ section, patch: patch as Record<string, unknown> })
        }
      />,
    ),
  );
  return { host, root };
}

setupDom();

const CONTROL = '[data-testid="settings-agent-notification-levels"]';

const DAEMON_ROWS: AgentLevelChoice[] = [
  { level: "verbose", enabled: true },
  { level: "action_needed", enabled: true },
  { level: "failure", enabled: false },
];

async function testRowsMirrorTheDaemonList() {
  const updates: UpdateCall[] = [];
  const { host, root } = await render(configWith(DAEMON_ROWS), updates);
  const control = host.querySelector(CONTROL);
  assert(control, "the control renders when the daemon shipped level rows");

  const toggles = control.querySelectorAll("label");
  assert(
    toggles.length === DAEMON_ROWS.length,
    `one row per daemon level, got ${toggles.length}`,
  );
  for (const row of DAEMON_ROWS) {
    assert(
      control.textContent?.includes(levelLabel(row.level)),
      `the user can see that ${row.level} may reach him`,
    );
  }
  await act(async () => root.unmount());
  host.remove();
}

async function testAbsentRowsRenderNothing() {
  const updates: UpdateCall[] = [];
  const { host, root } = await render(configWith(undefined), updates);
  assert(
    host.querySelector(CONTROL) === null,
    "with no rows from the daemon there is nothing to claim the user controls",
  );
  await act(async () => root.unmount());
  host.remove();
}

async function testTogglingOneLevelPatchesOnlyThatRow() {
  const updates: UpdateCall[] = [];
  const { host, root } = await render(configWith(DAEMON_ROWS), updates);
  const control = host.querySelector(CONTROL);
  assert(control, "control present");

  const first = control.querySelector('button[role="switch"]') as HTMLButtonElement | null;
  assert(first, "each row is an accessible switch");
  assert(
    first.getAttribute("aria-label")?.includes("Verbose"),
    "the switch names the level it governs",
  );
  await act(async () => first.click());

  assert(updates.length === 1, `one patch per toggle, got ${updates.length}`);
  assert(updates[0].section === "notifications", "the patch targets the notifications section");
  assert(
    JSON.stringify(updates[0].patch.agent_levels) ===
      JSON.stringify([
        { level: "verbose", enabled: false },
        { level: "action_needed", enabled: true },
        { level: "failure", enabled: false },
      ]),
    `only the toggled row changes, got ${JSON.stringify(updates[0].patch.agent_levels)}`,
  );
  await act(async () => root.unmount());
  host.remove();
}

function testPureHelpers() {
  const flipped = setLevelEnabled(DAEMON_ROWS, "failure", true);
  assert(
    JSON.stringify(flipped) ===
      JSON.stringify([
        { level: "verbose", enabled: true },
        { level: "action_needed", enabled: true },
        { level: "failure", enabled: true },
      ]),
    `setLevelEnabled must patch one row in place, got ${JSON.stringify(flipped)}`,
  );
  assert(
    JSON.stringify(setLevelEnabled(DAEMON_ROWS, "progress", true)) ===
      JSON.stringify(DAEMON_ROWS),
    "a level the daemon never shipped cannot be added from the UI",
  );
  assert(levelLabel("action_needed") === "Action needed", "labels are derived, not listed");
}

async function main() {
  await testRowsMirrorTheDaemonList();
  await testAbsentRowsRenderNothing();
  await testTogglingOneLevelPatchesOnlyThatRow();
  testPureHelpers();
  console.log("PASS AgentNotificationLevels desktop gate control");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
