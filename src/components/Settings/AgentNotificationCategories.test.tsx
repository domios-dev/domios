// Run: npx tsx src/components/Settings/AgentNotificationCategories.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  AgentNotificationCategories,
  categoryLabel,
  setCategoryEnabled,
} from "./AgentNotificationCategories";
import type { AgentCategoryChoice, AppConfig } from "../../lib/types";

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

function configWith(agent_categories?: AgentCategoryChoice[]): AppConfig {
  return { notifications: { agent_categories } } as unknown as AppConfig;
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
      <AgentNotificationCategories
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

const CONTROL = '[data-testid="settings-agent-notification-categories"]';

const DAEMON_ROWS: AgentCategoryChoice[] = [
  { category: "milestone", enabled: true },
  { category: "blocker", enabled: true },
  { category: "failure", enabled: false },
];

async function testRowsMirrorTheDaemonList() {
  const updates: UpdateCall[] = [];
  const { host, root } = await render(configWith(DAEMON_ROWS), updates);
  const control = host.querySelector(CONTROL);
  assert(control, "the control renders when the daemon shipped category rows");

  const toggles = control.querySelectorAll("label");
  assert(
    toggles.length === DAEMON_ROWS.length,
    `one row per daemon category, got ${toggles.length}`,
  );
  for (const row of DAEMON_ROWS) {
    assert(
      control.textContent?.includes(categoryLabel(row.category)),
      `the user can see that ${row.category} may reach him`,
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

async function testTogglingOneCategoryPatchesOnlyThatRow() {
  const updates: UpdateCall[] = [];
  const { host, root } = await render(configWith(DAEMON_ROWS), updates);
  const control = host.querySelector(CONTROL);
  assert(control, "control present");

  const first = control.querySelector('button[role="switch"]') as HTMLButtonElement | null;
  assert(first, "each row is an accessible switch");
  assert(
    first.getAttribute("aria-label")?.includes("Milestone"),
    "the switch names the category it governs",
  );
  await act(async () => first.click());

  assert(updates.length === 1, `one patch per toggle, got ${updates.length}`);
  assert(updates[0].section === "notifications", "the patch targets the notifications section");
  assert(
    JSON.stringify(updates[0].patch.agent_categories) ===
      JSON.stringify([
        { category: "milestone", enabled: false },
        { category: "blocker", enabled: true },
        { category: "failure", enabled: false },
      ]),
    `only the toggled row changes, got ${JSON.stringify(updates[0].patch.agent_categories)}`,
  );
  await act(async () => root.unmount());
  host.remove();
}

function testPureHelpers() {
  const flipped = setCategoryEnabled(DAEMON_ROWS, "failure", true);
  assert(
    JSON.stringify(flipped) ===
      JSON.stringify([
        { category: "milestone", enabled: true },
        { category: "blocker", enabled: true },
        { category: "failure", enabled: true },
      ]),
    `setCategoryEnabled must patch one row in place, got ${JSON.stringify(flipped)}`,
  );
  assert(
    JSON.stringify(setCategoryEnabled(DAEMON_ROWS, "progress", true)) ===
      JSON.stringify(DAEMON_ROWS),
    "a category the daemon never shipped cannot be added from the UI",
  );
  assert(categoryLabel("milestone") === "Milestone", "labels are derived, not listed");
}

async function main() {
  await testRowsMirrorTheDaemonList();
  await testAbsentRowsRenderNothing();
  await testTogglingOneCategoryPatchesOnlyThatRow();
  testPureHelpers();
  console.log("PASS AgentNotificationCategories desktop gate control");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
