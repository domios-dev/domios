
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { NotificationDestinations, reachesNothing } from "./NotificationDestinations";
import {
  SETTINGS_SECTIONS,
  resolveSettingsLink,
} from "./sections";
import { levelTone } from "../../lib/notifyLevels";
import { SwitchTile } from "./shared";
import {
  destinationColumns,
  isRouted,
  localDestinations,
  pushDestinations,
  setLevelDestination,
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
  Object.assign(window, { __CODETERM_DAEMON__: { origin: /^https?:/.test(window.location.origin) ? window.location.origin : "http://127.0.0.1:7686", token: "test-token" } });
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

/** The roster the daemon would answer with. The grid draws whatever is here,
 *  so this fixture is the only place the test knows a channel exists. */
const DAEMON_CHANNELS: PushChannelDescriptor[] = [
  { id: "telegram", label: "Telegram", control_available: true },
];
const COLUMNS = destinationColumns(DAEMON_CHANNELS);
/** The two fixed surfaces, which keep a toggle each, and the registry side,
 *  which is one picker per level however many members it has. */
const LOCAL = localDestinations(COLUMNS);
const PUSH = pushDestinations(COLUMNS);
/** Every row also carries its own chime switch. It is NOT a destination column —
 *  the chime is not a place a message goes — so it is counted separately here. */
const SWITCHES_PER_ROW = LOCAL.length + 1;

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

/** Two admitted ids, so the channel's audience is a real question and its
 *  surface is drawn — the case the notifications home is asserted against. */
const ADMITTED = ["100000001", "100000002"] as const;

function configWith(agent_levels?: AgentLevelChoice[]): AppConfig {
  return {
    notifications: { agent_levels },
    integrations: { telegram_enabled: true, telegram_allowed_users: [...ADMITTED] },
  } as unknown as AppConfig;
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

/** One per level: the whole push side, whatever the roster holds. */
function pushControls(scope: Element): HTMLButtonElement[] {
  return Array.from(
    scope.querySelectorAll('[data-testid="settings-destination-picker-trigger"]'),
  ) as HTMLButtonElement[];
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
    switches(grownGrid).length === grown.length * SWITCHES_PER_ROW,
    "the local surfaces and the chime are one toggle per level each, never a fixed cell count",
  );
  assert(
    pushControls(grownGrid).length === grown.length,
    "and the push side is one control per level, whatever the registry holds",
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

  const desktopIndex = LOCAL.findIndex((column) => column.id === "desktop");
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
    allOff = setLevelDestination(allOff, "verbose", destination, false);
  }
  assert(reachesNothing(allOff[0], COLUMNS), "every column closed is expressible");
  await view.draw(configWith(allOff));
  const closed = view.host.querySelector(`${ROW}[data-notification-level="verbose"]`);
  assert(closed, "a level that reaches nothing still renders its row");
  assert(
    switches(closed)
      .slice(0, LOCAL.length)
      .every((control) => control.getAttribute("aria-checked") === "false"),
    "all-off round-trips instead of being refused or reset",
  );
  assert(
    closed.textContent?.includes("panel only"),
    "the user is told what all-off means rather than being blocked",
  );
  assert(
    switches(closed).length === SWITCHES_PER_ROW,
    "every local surface stays togglable when the level reaches nothing, and so does the chime",
  );
  assert(
    pushControls(closed).length === (PUSH.length > 0 ? 1 : 0),
    "and the push side is still one control offering the registry",
  );
  await view.done();
}

/** 3. The master switch removes the indicator, and is a different config state
 *  from every level having its one routable surface closed. */
async function masterSwitchHidesTheSurfaceAndStaysDistinguishable() {
  // The two states are different config, not the same config twice.
  const hidden = { show_bell: false, agent_levels: DAEMON_ROWS.map((row) => ({ ...row })) };
  const columnsClosed = {
    show_bell: true,
    agent_levels: DAEMON_ROWS.map((row) => ({ ...row, desktop: false })),
  };
  assert(
    JSON.stringify(hidden) !== JSON.stringify(columnsClosed),
    "a hidden indicator and an unreachable column must not serialise alike",
  );
  assert(
    hidden.agent_levels.every((row) => row.desktop),
    "hiding the indicator routes nothing away from anything",
  );
  assert(
    columnsClosed.show_bell && columnsClosed.agent_levels.every((row) => !row.desktop),
    "closing every column leaves the indicator drawn and the log complete",
  );
  assert(
    columnsClosed.agent_levels.every((row) => row.bell === true),
    "and neither state moves the retained bell field, which routes nothing",
  );

  // The switch itself: the shared row, writing the section that already exists.
  const updates: UpdateCall[] = [];
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <SwitchTile
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
  const unnamed = { level: "verbose", bell: true, desktop: true } as AgentLevelChoice;
  assert(!isRouted(unnamed, "telegram"), "an unnamed channel column is off");
  assert(isRouted(unnamed, "desktop"), "the column it did name keeps the answer it gave");
  assert(
    !reachesNothing(unnamed, COLUMNS),
    "the toast is still standing, so the level interrupts somewhere",
  );
  assert(
    reachesNothing({ level: "verbose" } as AgentLevelChoice, COLUMNS),
    "a row that named no column at all interrupts nowhere, and is still kept",
  );
  assert(
    reachesNothing({ level: "verbose", bell: true } as AgentLevelChoice, COLUMNS),
    "and the retained bell field is not a column, so it cannot rescue that row",
  );

  // And the grid draws that reading rather than a second opinion about it.
  const drawn = await renderGrid(configWith([unnamed]), updates);
  const row = drawn.host.querySelector(ROW);
  assert(row, "the row is drawn");
  const states = switches(row)
    .slice(0, LOCAL.length)
    .map((control) => control.getAttribute("aria-checked"));
  assert(
    states.join(",") === LOCAL.map((column) => String(isRouted(unnamed, column.id))).join(","),
    `the local surfaces are drawn as stored, got ${states.join(",")}`,
  );
  assert(
    pushControls(row).length === 1 && pushControls(row)[0].getAttribute("aria-expanded") === "false",
    "and the unnamed channel is behind one closed picker rather than a row of its own",
  );
  await drawn.done();
}

/** The columns cannot drift from the row shape the daemon serialises, and the
 *  one field that is deliberately NOT a column stays out of it. */
async function destinationsMatchTheRowShape() {
  const answered: AgentLevelChoice = { ...open("verbose"), sound: true };
  const shape = Object.keys(answered).filter((key) => key !== "level");
  const columns = COLUMNS.map((destination) => destination.id);
  assert(
    columns.every((id) => shape.includes(id)),
    `every column is a field of the row, got ${JSON.stringify(columns)}`,
  );
  assert(
    JSON.stringify(shape.filter((key) => !columns.includes(key))) ===
      JSON.stringify(["bell", "sound"]),
    `the fields that are not columns are the retained bell and the chime, got ${JSON.stringify(shape)}`,
  );
  assert(
    !LOCAL_DESTINATIONS.some((surface) => surface.id === ("sound" as string)),
    "the chime is a field of the row but never a destination: it is not a place a message goes",
  );
  assert(
    !LOCAL_DESTINATIONS.some((surface) => surface.id === ("bell" as string)),
    "the panel is not offered as a destination: it keeps every record either way",
  );
  assert(
    !LOCAL_DESTINATIONS.some((surface) => DAEMON_CHANNELS.some((c) => c.id === surface.id)),
    "a channel is never restated as a local surface",
  );
  const channel = PUSH[0];
  const local = LOCAL[0];
  const setter = setLevelDestination(DAEMON_ROWS, "action_needed", channel, false);
  assert(
    !(channel.id in setter[1]) && setter[1].bell && setter[1].desktop,
    "closing a channel drops its key rather than storing a false the daemon reads the same way",
  );
  assert(
    setLevelDestination(DAEMON_ROWS, "action_needed", local, false)[1][local.id] === false,
    "while a local surface keeps an explicit answer in its own named field",
  );
  assert(
    DAEMON_ROWS.every(
      (row) => setLevelDestination(DAEMON_ROWS, row.level, channel, true)[0].bell === true,
    ),
    "and no edit from this surface ever moves the retained bell field",
  );
  assert(
    JSON.stringify(setLevelDestination(DAEMON_ROWS, "progress", local, false)) ===
      JSON.stringify(DAEMON_ROWS),
    "a level the daemon never shipped cannot be added from the UI",
  );
  }

async function theLegacyDeepLinkResolvesToTheNotificationsSection() {
  assert(
    resolveSettingsLink("notifications")?.section === "notifications",
    `the notifications deep link resolves to itself, got ${resolveSettingsLink("notifications")?.section}`,
  );
  assert(
    resolveSettingsLink("notifications")?.section !== "agents",
    "it must never land on the page the controls just left",
  );
  assert(resolveSettingsLink(undefined)?.section === undefined, "no section still means the section grid");
  for (const section of SETTINGS_SECTIONS) {
    assert(
      resolveSettingsLink(section.id)?.section === section.id,
      `${section.id} maps to itself, so the alias rewrites nothing`,
    );
  }

  // The target is a real, rendered destination rather than a dangling id.
  assert(
    SETTINGS_SECTIONS.some((section) => section.id === resolveSettingsLink("notifications")?.section),
    "the resolved id is a section the nav actually offers",
  );
  const agentsEntry = SETTINGS_SECTIONS.find((section) => section.id === "agents");
  assert(agentsEntry, "the agents entry still exists");
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
  await check("destinations match the serialised row shape", destinationsMatchTheRowShape);
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
