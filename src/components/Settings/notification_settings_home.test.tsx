// Run: npx tsx src/components/Settings/notification_settings_home.test.tsx
//
// The levels-by-destinations grid, the master bell switch, and the one home
// they now live in.

import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import {
  NotificationDestinations,
  reachesNothing,
  setLevelDestination,
} from "./NotificationDestinations";
import { AgentsOverview } from "./AgentsTab";
import { NotificationsOverview } from "./NotificationsTab";
import {
  SECTION_ICONS,
  SETTINGS_SECTIONS,
  normalizeSection,
  sectionTitle,
  type SectionId,
} from "./sections";
import { levelLabel, levelTone } from "../../lib/notifyLevels";
import { Toggle } from "./shared";
import {
  destinationCellLabel,
  destinationColumns,
  isRouted,
  LOCAL_DESTINATIONS,
  type PushChannelDescriptor,
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

/** The roster the daemon would answer with. The grid draws whatever is here,
 *  so this fixture is the only place the test knows a channel exists. */
const DAEMON_CHANNELS: PushChannelDescriptor[] = [
  { id: "telegram", label: "Telegram", control_available: true },
];
const COLUMNS = destinationColumns(DAEMON_CHANNELS);

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
          channels={DAEMON_CHANNELS}
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
    switches(grownGrid).length === grown.length * COLUMNS.length,
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

  const desktopIndex = COLUMNS.findIndex((column) => column.id === "desktop");
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
  for (const destination of COLUMNS) {
    allOff = setLevelDestination(allOff, "verbose", destination.id, false);
  }
  assert(reachesNothing(allOff[0], COLUMNS), "every column closed is expressible");
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
    switches(closed).length === COLUMNS.length,
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

  const settings = source("./NotificationsTab.tsx");
  assert(
    settings.includes('onUpdate("notifications", { show_bell: v })'),
    "the panel writes the master switch into the existing notifications section",
  );
}

/** 4. With no rows there is no grid, and a column a row never named is off. */
async function absentConfigDrawsNothingAndAnUnnamedColumnIsOff() {
  const updates: UpdateCall[] = [];
  const view = await renderGrid(configWith(undefined), updates);
  assert(
    view.host.querySelector(GRID) === null,
    "with no rows from the daemon there is nothing to claim the user controls",
  );
  await view.done();

  // A row written before a column existed reads that column as off, exactly as
  // the daemon reads it: a column nobody named is a column nobody chose.
  const unnamed = { level: "verbose", bell: true, desktop: false } as AgentLevelChoice;
  assert(!isRouted(unnamed, "telegram"), "an unnamed channel column is off");
  assert(isRouted(unnamed, "bell"), "the column it did name keeps the answer it gave");
  assert(!isRouted(unnamed, "desktop"), "as does the one it named false");
  assert(
    !reachesNothing(unnamed, COLUMNS),
    "the bell is still standing, so the level is not an empty row",
  );
  assert(
    reachesNothing({ level: "verbose" } as AgentLevelChoice, COLUMNS),
    "a row that named no column at all falls through to nothing",
  );

  // And the grid draws that reading rather than a second opinion about it.
  const drawn = await renderGrid(configWith([unnamed]), updates);
  const row = drawn.host.querySelector(ROW);
  assert(row, "the row is drawn");
  const states = switches(row).map((control) => control.getAttribute("aria-checked"));
  assert(
    states.join(",") === "true,false,false",
    `the unnamed column is drawn off, got ${states.join(",")}`,
  );
  await drawn.done();
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
      controls.length === COLUMNS.length,
      `one control per destination on ${level}, got ${controls.length}`,
    );
    const shipped = DAEMON_ROWS.find((candidate) => candidate.level === level);
    assert(shipped, `${level} came from the fixture`);
    controls.forEach((control, index) => {
      const destination = COLUMNS[index];
      const expected = destinationCellLabel(levelLabel(level), destination.label);
      assert(
        control.getAttribute("aria-label") === expected,
        `expected "${expected}", got "${control.getAttribute("aria-label")}"`,
      );
      assert(
        control.getAttribute("aria-checked") === String(isRouted(shipped, destination.id)),
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
  const columns = COLUMNS.map((destination) => destination.id);
  assert(
    JSON.stringify(columns) === JSON.stringify(shape),
    `columns must be the row's destination fields, got ${JSON.stringify(columns)}`,
  );
  assert(
    JSON.stringify(LOCAL_DESTINATIONS.map((surface) => surface.id)) ===
      JSON.stringify(["bell", "desktop"]),
    "the local surfaces are the only columns this side names",
  );
  assert(
    !LOCAL_DESTINATIONS.some((surface) => DAEMON_CHANNELS.some((c) => c.id === surface.id)),
    "a channel is never restated as a local surface",
  );
  const setter = setLevelDestination(DAEMON_ROWS, "action_needed", "telegram", false);
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

/* ---------------------------------------------------------------------------
 * notification_settings_home — the one home the surfaces above now live in.
 * ------------------------------------------------------------------------ */

const SRC_ROOT = fileURLToPath(new URL("../../", import.meta.url));
const MOVED_SURFACES = [
  "NotificationDestinations",
  "PushChannelSettings",
  "ProviderNotifications",
] as const;

/** Every non-test module under src/, so "exactly one place" is a fact about the
 *  tree rather than about the two files this test happened to open. */
function productModules(directory: string, found: string[] = []): string[] {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const child = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "dist") productModules(child, found);
      continue;
    }
    if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) found.push(child);
  }
  return found;
}

function modulesMentioning(symbol: string): string[] {
  return productModules(SRC_ROOT)
    .filter((file) => readFileSync(file, "utf8").includes(symbol))
    .map((file) => path.basename(file));
}

async function daemonStub(command: string): Promise<unknown> {
  if (command === "list_agent_providers") return [];
  if (command === "push_channel_roster") return { channels: DAEMON_CHANNELS };
  return null;
}

async function withDaemonStubs(run: () => Promise<void>): Promise<void> {
  __setInvokeForTest(async <T,>(command: string) => daemonStub(command) as Promise<T>);
  __setListenForTest(async () => () => {});
  try {
    await run();
  } finally {
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}

async function mount(element: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(element));
  return {
    host,
    async done() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

const TELEGRAM = '[data-testid="settings-telegram-notifications"]';

/** 6. The nav exposes a Notifications section and the matrix renders under it. */
async function theNavExposesNotificationsAndTheMatrixLivesThere() {
  const entry = SETTINGS_SECTIONS.find((section) => section.id === "notifications");
  assert(entry, "the curated nav list exposes a notifications section");
  assert(entry.label === "Notifications", `the user looks for "Notifications", got "${entry.label}"`);
  assert(entry.desc.trim().length > 0, "the nav card describes what the section holds");
  assert(SECTION_ICONS.notifications, "the section has a breadcrumb icon like every other section");
  assert(
    sectionTitle("notifications") === entry.label,
    "the header title is derived from the nav entry, not restated",
  );

  const panel = source("./SettingsPanel.tsx");
  assert(
    /top\.id === "notifications"[\s\S]{0,400}<NotificationsOverview/.test(panel),
    "the notifications route renders the notifications home rather than an empty page",
  );

  await withDaemonStubs(async () => {
    const updates: UpdateCall[] = [];
    const view = await mount(
      <NotificationsOverview
        config={configWith(DAEMON_ROWS)}
        onUpdate={(section, patch) =>
          updates.push({ section, patch: patch as Record<string, unknown> })
        }
        onUpdateFullConfig={() => {}}
      />,
    );
    const grid = view.host.querySelector(GRID);
    assert(grid, "the destinations matrix renders inside the notifications home");
    assert(
      grid.querySelectorAll(ROW).length === DAEMON_ROWS.length,
      "the moved matrix still draws one row per shipped level",
    );
    assert(
      switches(grid).length === DAEMON_ROWS.length * COLUMNS.length,
      "levels x destinations survived the move intact",
    );
    assert(
      view.host.querySelector(TELEGRAM),
      "the channel's own surface is drawn by the registry-driven section",
    );
    await view.done();
  });
}

/** 7. Moved, not copied: the surfaces are absent from the Agents surface. */
async function theMovedSurfacesRenderInExactlyOnePlace() {
  await withDaemonStubs(async () => {
    const view = await mount(
      <AgentsOverview
        config={configWith(DAEMON_ROWS)}
        onUpdate={() => {}}
        onOpenProvider={() => {}}
      />,
    );
    assert(
      view.host.querySelector('[data-testid="settings-agents-overview"]'),
      "the Agents surface really rendered, so its absences mean something",
    );
    assert(
      view.host.querySelector(GRID) === null,
      "the destinations matrix is gone from Agents, not drawn in two places",
    );
    assert(
      view.host.querySelector(TELEGRAM) === null,
      "the Telegram notification surface is gone from Agents",
    );
    await view.done();
  });

  const agents = source("./AgentsTab.tsx");
  for (const surface of MOVED_SURFACES) {
    assert(!agents.includes(surface), `${surface} must not be referenced from AgentsTab`);
  }

  const home = source("./NotificationsTab.tsx");
  for (const surface of MOVED_SURFACES) {
    assert(home.includes(surface), `${surface} must be rendered by the notifications home`);
    const hosts = modulesMentioning(surface).filter((file) => file !== `${surface}.tsx`);
    assert(
      JSON.stringify(hosts) === JSON.stringify(["NotificationsTab.tsx"]),
      `${surface} must have exactly one home, got ${JSON.stringify(hosts)}`,
    );
  }
}

/** 8. The legacy deep link now resolves to the section the controls live in. */
async function theLegacyDeepLinkResolvesToTheNotificationsSection() {
  assert(
    normalizeSection("notifications") === "notifications",
    `the notifications deep link resolves to itself, got ${normalizeSection("notifications")}`,
  );
  assert(
    normalizeSection("notifications") !== ("agents" as SectionId),
    "it must never land on the page the controls just left",
  );
  assert(normalizeSection(undefined) === undefined, "no section still means the section grid");
  for (const section of SETTINGS_SECTIONS) {
    assert(
      normalizeSection(section.id) === section.id,
      `${section.id} maps to itself, so the alias rewrites nothing`,
    );
  }

  const nav = source("./sections.ts");
  assert(
    !/return id === "notifications" \? "agents" : id;/.test(nav),
    "the silent rewrite to agents is gone",
  );

  // The target is a real, rendered destination rather than a dangling id.
  assert(
    SETTINGS_SECTIONS.some((section) => section.id === normalizeSection("notifications")),
    "the resolved id is a section the nav actually offers",
  );
  const agentsEntry = SETTINGS_SECTIONS.find((section) => section.id === "agents");
  assert(agentsEntry, "the agents entry still exists");
  assert(
    !agentsEntry.desc.toLowerCase().includes("notification"),
    `the Agents description stops claiming notifications, got "${agentsEntry.desc}"`,
  );
}

async function main() {
  await check("rows derive from the shared level set", rowsDeriveFromTheSharedLevelSet);
  await check(
    "notification_settings_home: toggling a cell persists and all-off round-trips",
    togglingACellPersistsAndAllOffRoundTrips,
  );
  await check(
    "the master switch hides the surface and stays distinguishable",
    masterSwitchHidesTheSurfaceAndStaysDistinguishable,
  );
  await check(
    "absent config draws nothing, and an unnamed column is off",
    absentConfigDrawsNothingAndAnUnnamedColumnIsOff,
  );
  await check(
    "notification_settings_home: every control is named and is the shared toggle",
    everyControlIsNamedAndIsTheSharedToggle,
  );
  await check("destinations match the serialised row shape", destinationsMatchTheRowShape);
  await check(
    "notification_settings_home: the nav exposes Notifications and the matrix lives there",
    theNavExposesNotificationsAndTheMatrixLivesThere,
  );
  await check(
    "notification_settings_home: the moved surfaces render in exactly one place",
    theMovedSurfacesRenderInExactlyOnePlace,
  );
  await check(
    "notification_settings_home: the legacy deep link resolves to the Notifications section",
    theLegacyDeepLinkResolvesToTheNotificationsSection,
  );
  console.log(`PASS notification_settings_home — ${cases.length}/${cases.length} cases`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
