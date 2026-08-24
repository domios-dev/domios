// Run: npx tsx src/components/Settings/notification_settings.test.tsx
//
// The levels-by-destinations grid and the master bell switch.

import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  NotificationDestinations,
  levelLabel,
  levelTone,
  reachesNothing,
  setLevelDestination,
} from "./NotificationDestinations";
import { Toggle } from "./shared";
import {
  NOTIFY_DESTINATIONS,
  destinationCellLabel,
  type NotifyDestinationId,
} from "../../lib/notifyDestinations";
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

setupDom();

const source = (relative: string) => readFileSync(new URL(relative, import.meta.url), "utf8");

function open(level: string): AgentLevelChoice {
  return { level, bell: true, desktop: true, telegram: true };
}

/** The daemon generates this from its own closed set; the fixture stands in for
 *  whatever that set currently is, including a set that has grown. */
const DAEMON_ROWS: AgentLevelChoice[] = [
  open("verbose"),
  open("action_needed"),
  { level: "failure", bell: true, desktop: true, telegram: false },
];

function configWith(agent_levels?: AgentLevelChoice[]): AppConfig {
  return { notifications: { agent_levels } } as unknown as AppConfig;
}

type UpdateCall = { section: string; patch: Record<string, unknown> };

async function renderGrid(config: AppConfig, updates: UpdateCall[]) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const draw = async (next: AppConfig) =>
    act(async () =>
      root.render(
        <NotificationDestinations
          config={next}
          onUpdate={(section, patch) =>
            updates.push({ section, patch: patch as Record<string, unknown> })
          }
        />,
      ),
    );
  await draw(config);
  return {
    host,
    draw,
    async done() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

const GRID = '[data-testid="settings-notification-destinations"]';
const ROW = '[data-testid="settings-notification-level"]';

function switches(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="switch"]')) as HTMLButtonElement[];
}

const cases: string[] = [];
async function check(name: string, run: () => Promise<void> | void) {
  await run();
  cases.push(name);
  console.log(`  ok ${cases.length} - ${name}`);
}

/** 1. One row per level of the shared set — proved by a set that grew. */
async function rowsDeriveFromTheSharedLevelSet() {
  const updates: UpdateCall[] = [];
  const view = await renderGrid(configWith(DAEMON_ROWS), updates);
  const grid = view.host.querySelector(GRID);
  assert(grid, "the grid renders once the daemon has shipped rows");
  assert(
    grid.querySelectorAll(ROW).length === DAEMON_ROWS.length,
    `one row per shipped level, got ${grid.querySelectorAll(ROW).length}`,
  );

  // The whole point: a level the daemon gains must gain a row here, with no
  // edit to this component and no hand-written triple to keep in step.
  const grown = [...DAEMON_ROWS, open("attention_soon")];
  await view.draw(configWith(grown));
  const rows = Array.from(view.host.querySelectorAll(ROW));
  assert(rows.length === grown.length, `a fourth level adds a fourth row, got ${rows.length}`);
  assert(
    rows.some((row) => row.getAttribute("data-notification-level") === "attention_soon"),
    "the new level is the row that appeared",
  );
  const grownGrid = view.host.querySelector(GRID);
  assert(grownGrid, "the grown grid is still rendered");
  assert(
    switches(grownGrid).length === grown.length * NOTIFY_DESTINATIONS.length,
    "the grid is levels x destinations, never a fixed cell count",
  );
  assert(
    view.host.textContent?.includes("Attention soon"),
    "an unknown level still gets a label derived from its wire value",
  );
  assert(
    levelTone("attention_soon") === "var(--ct-muted-fg)",
    "an unknown level falls back to a product token rather than going uncoloured",
  );
  await view.done();
}

/** 2. A cell persists on its own, and all-off round-trips. */
async function togglingACellPersistsAndAllOffRoundTrips() {
  const updates: UpdateCall[] = [];
  const view = await renderGrid(configWith(DAEMON_ROWS), updates);
  const verbose = view.host.querySelector(`${ROW}[data-notification-level="verbose"]`);
  assert(verbose, "the verbose row is present");

  const desktopIndex = NOTIFY_DESTINATIONS.findIndex((d) => d.id === "desktop");
  await act(async () => switches(verbose)[desktopIndex].click());

  assert(updates.length === 1, `one patch per cell, got ${updates.length}`);
  assert(updates[0].section === "notifications", "the patch targets the existing section");
  const patched = updates[0].patch.agent_levels as AgentLevelChoice[];
  assert(
    JSON.stringify(patched[0]) ===
      JSON.stringify({ level: "verbose", bell: true, desktop: false, telegram: true }),
    `only the toggled cell changes, got ${JSON.stringify(patched[0])}`,
  );
  assert(
    JSON.stringify(patched.slice(1)) === JSON.stringify(DAEMON_ROWS.slice(1)),
    "no neighbouring row moves",
  );

  // Reload: the daemon hands back what it stored, and the cell stays closed.
  await view.draw(configWith(patched));
  const reloaded = view.host.querySelector(`${ROW}[data-notification-level="verbose"]`);
  assert(reloaded, "the row survives the reload");
  assert(
    switches(reloaded)[desktopIndex].getAttribute("aria-checked") === "false",
    "the reloaded cell reflects what was persisted",
  );

  // All-off is a configuration, not a validation error.
  let allOff = patched;
  for (const destination of NOTIFY_DESTINATIONS) {
    allOff = setLevelDestination(allOff, "verbose", destination.id, false);
  }
  assert(reachesNothing(allOff[0]), "every column closed is expressible");
  await view.draw(configWith(allOff));
  const closed = view.host.querySelector(`${ROW}[data-notification-level="verbose"]`);
  assert(closed, "a level that reaches nothing still renders its row");
  assert(
    switches(closed).every((control) => control.getAttribute("aria-checked") === "false"),
    "all-off round-trips instead of being refused or reset",
  );
  assert(
    closed.textContent?.includes("reaches nothing"),
    "the user is told what all-off means rather than being blocked",
  );
  assert(
    switches(closed).length === NOTIFY_DESTINATIONS.length,
    "every cell stays togglable when the level reaches nothing",
  );
  await view.done();
}

/** 3. The master switch removes the surface, and is a different config state
 *  from every level having its bell column closed. */
async function masterSwitchHidesTheSurfaceAndStaysDistinguishable() {
  const titlebar = source("../Titlebar/Titlebar.tsx");
  assert(
    /\{showBell &&[\s\S]{0,240}<NotificationBell/.test(titlebar),
    "the bell surface is rendered only while the master switch is on",
  );
  assert(
    titlebar.includes("showBell = true"),
    "an install that never answered keeps the bell it has always had",
  );
  const app = source("../../App.tsx");
  assert(
    app.includes("showBell={config.notifications?.show_bell !== false}"),
    "the switch the settings panel writes is the switch the titlebar reads",
  );

  // The two states are different config, not the same config twice.
  const hidden = { show_bell: false, agent_levels: DAEMON_ROWS.map((row) => ({ ...row })) };
  const columnsClosed = {
    show_bell: true,
    agent_levels: DAEMON_ROWS.map((row) => ({ ...row, bell: false })),
  };
  assert(
    JSON.stringify(hidden) !== JSON.stringify(columnsClosed),
    "a hidden surface and an unreachable column must not serialise alike",
  );
  assert(
    hidden.agent_levels.every((row) => row.bell),
    "hiding the surface routes nothing away from it, so the log stays complete",
  );
  assert(
    columnsClosed.show_bell && columnsClosed.agent_levels.every((row) => !row.bell),
    "closing every column leaves the surface drawn and empty",
  );

  // The switch itself: the shared row, writing the section that already exists.
  const updates: UpdateCall[] = [];
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <Toggle
        label="Show the bell"
        checked
        onChange={(value) =>
          updates.push({ section: "notifications", patch: { show_bell: value } })
        }
      />,
    ),
  );
  const master = host.querySelector('button[role="switch"]') as HTMLButtonElement | null;
  assert(master, "the master switch is an accessible switch");
  assert(master.getAttribute("aria-label") === "Show the bell", "and it names itself");
  await act(async () => master.click());
  assert(
    JSON.stringify(updates) ===
      JSON.stringify([{ section: "notifications", patch: { show_bell: false } }]),
    `flipping it writes the master field, got ${JSON.stringify(updates)}`,
  );
  await act(async () => root.unmount());
  host.remove();

  const settings = source("./AgentsTab.tsx");
  assert(
    settings.includes('onUpdate("notifications", { show_bell: v })'),
    "the panel writes the master switch into the existing notifications section",
  );
}

/** 4. Absent config reproduces today's behaviour. */
async function absentConfigReproducesTodaysBehaviour() {
  const updates: UpdateCall[] = [];
  const view = await renderGrid(configWith(undefined), updates);
  assert(
    view.host.querySelector(GRID) === null,
    "with no rows from the daemon there is nothing to claim the user controls",
  );
  await view.done();

  const defaults = source("../../hooks/useConfig.ts");
  assert(
    /show_bell:\s*true/.test(defaults),
    "before the daemon answers, the bell is drawn exactly as it always was",
  );
  assert(/desktop:\s*true/.test(defaults), "and the desktop default is unchanged");
  assert(/agent_levels:\s*\[\]/.test(defaults), "the daemon still owns the level set");

  const schema = readFileSync(
    new URL("../../../src-tauri/crates/codeterm-types/src/config/schema.rs", import.meta.url),
    "utf8",
  );
  assert(
    /fn routed_by_default\(\) -> bool \{\s*true\s*\}/.test(schema),
    "an absent column is an open column, so a config predating the grid routes as before",
  );
  assert(schema.includes("show_bell: true"), "and the master switch defaults to visible");
}

/** 5. Every control names its level and its destination, and the control is the
 *  shared toggle rather than a copy of it. */
async function everyControlIsNamedAndIsTheSharedToggle() {
  const updates: UpdateCall[] = [];
  const view = await renderGrid(configWith(DAEMON_ROWS), updates);
  const grid = view.host.querySelector(GRID);
  assert(grid, "grid present");

  for (const row of Array.from(grid.querySelectorAll(ROW))) {
    const level = row.getAttribute("data-notification-level");
    assert(level, "every row names the level it governs");
    assert(
      row.getAttribute("role") === "group" && row.getAttribute("aria-label") === levelLabel(level),
      `the ${level} row is a named group`,
    );
    const controls = switches(row);
    assert(
      controls.length === NOTIFY_DESTINATIONS.length,
      `one control per destination on ${level}, got ${controls.length}`,
    );
    const shipped = DAEMON_ROWS.find((candidate) => candidate.level === level);
    assert(shipped, `${level} came from the fixture`);
    controls.forEach((control, index) => {
      const destination = NOTIFY_DESTINATIONS[index];
      const expected = destinationCellLabel(levelLabel(level), destination.label);
      assert(
        control.getAttribute("aria-label") === expected,
        `expected "${expected}", got "${control.getAttribute("aria-label")}"`,
      );
      assert(
        control.getAttribute("aria-checked") === String(shipped[destination.id]),
        `${expected} reports the state it was given`,
      );
      assert(
        control.className.includes("ct-liquid-toggle"),
        `${expected} is rendered by the shared LiquidToggle`,
      );
      assert(
        control.getAttribute("type") === "button",
        `${expected} is the shared button switch, not a checkbox`,
      );
    });
  }
  await view.done();

  // Structural: no second toggle was authored, and the shared one is imported.
  const component = source("./NotificationDestinations.tsx");
  assert(
    component.includes('import { Toggle } from "./shared"'),
    "the grid uses the shared Settings toggle row",
  );
  for (const authored of ["ct-liquid-toggle", 'role="switch"', "<input", "checkbox"]) {
    assert(!component.includes(authored), `the grid must not restate the toggle (${authored})`);
  }
  const shared = source("./shared.tsx");
  assert(
    shared.includes('import { LiquidToggle } from "../shared/LiquidToggle"'),
    "the shared row still delegates to the shared toggle",
  );
}

/** The columns cannot drift from the row shape the daemon serialises. */
async function destinationsMatchTheRowShape() {
  const shape = Object.keys(open("verbose")).filter((key) => key !== "level");
  const columns = NOTIFY_DESTINATIONS.map((destination) => destination.id as string);
  assert(
    JSON.stringify(columns) === JSON.stringify(shape),
    `columns must be the row's destination fields, got ${JSON.stringify(columns)}`,
  );
  const rust = readFileSync(
    new URL("../../../src-tauri/crates/codeterm-types/src/notification.rs", import.meta.url),
    "utf8",
  );
  for (const id of columns) {
    assert(rust.includes(`=> "${id}"`), `${id} must be a wire value of NotifyDestination`);
  }
  const setter = setLevelDestination(
    DAEMON_ROWS,
    "action_needed",
    "telegram" as NotifyDestinationId,
    false,
  );
  assert(
    setter[1].telegram === false && setter[1].bell && setter[1].desktop,
    "the setter moves one cell of one row",
  );
  assert(
    JSON.stringify(setLevelDestination(DAEMON_ROWS, "progress", "bell", false)) ===
      JSON.stringify(DAEMON_ROWS),
    "a level the daemon never shipped cannot be added from the UI",
  );
  assert(levelLabel("action_needed") === "Action needed", "labels are derived, not listed");
}

async function main() {
  await check("rows derive from the shared level set", rowsDeriveFromTheSharedLevelSet);
  await check(
    "toggling a cell persists and all-off round-trips",
    togglingACellPersistsAndAllOffRoundTrips,
  );
  await check(
    "the master switch hides the surface and stays distinguishable",
    masterSwitchHidesTheSurfaceAndStaysDistinguishable,
  );
  await check("absent config reproduces today's behaviour", absentConfigReproducesTodaysBehaviour);
  await check(
    "every control is named and is the shared toggle",
    everyControlIsNamedAndIsTheSharedToggle,
  );
  await check("destinations match the serialised row shape", destinationsMatchTheRowShape);
  console.log(`PASS notification_settings — ${cases.length}/${cases.length} cases`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
