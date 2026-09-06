// The surface side of the push-channel registry: what the picker offers comes
// from the daemon's roster, so a channel it gains widens one control instead of

import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { NotificationDestinations, reachesNothing } from "./NotificationDestinations";
import { PushChannelSettings } from "./PushChannelSettings";
import { NotificationsOverview } from "./NotificationsTab";
import {
  destinationColumns,
  isRouted,
  LOCAL_DESTINATIONS,
  offerableChannels,
  pushDestinations,
  type PushChannelDescriptor,
} from "../../lib/notifyDestinations";
import { levelLabel } from "../../lib/notifyLevels";
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

const GRID = '[data-testid="settings-notification-destinations"]';
const ROW = '[data-testid="settings-notification-level"]';
const PICKER = '[data-testid="settings-destination-picker-trigger"]';
const TELEGRAM_EDITOR = '[data-testid="settings-channel-audience"][data-channel="telegram"]';
const DISCORD_EDITOR = '[data-testid="settings-channel-audience"][data-channel="discord"]';

const ONE_CHANNEL: PushChannelDescriptor[] = [
  { id: "telegram", label: "Telegram", control_available: true },
];

/** A second member the daemon can actually deliver through. */
const TWO_CARRIERS: PushChannelDescriptor[] = [
  ...ONE_CHANNEL,
  { id: "slack", label: "Slack", control_available: true },
];

/** Registered, listed, and with no transport in this build. */
const WITH_UNAVAILABLE: PushChannelDescriptor[] = [
  ...ONE_CHANNEL,
  { id: "discord", label: "Discord", control_available: false },
];

/** Rows as a config written before any channel existed: no channel key at all,
 *  which is a channel nobody opted into and therefore a channel nobody reaches. */
const ROWS: AgentLevelChoice[] = [
  { level: "verbose", bell: true, desktop: true },
  { level: "action_needed", bell: true, desktop: false },
  { level: "failure", bell: false, desktop: true },
];

/** Two admitted ids per channel, so an audience surface has something to ask. */
const ADMITTED = ["100000001", "100000002"] as const;

function configWith(agent_levels: AgentLevelChoice[]): AppConfig {
  return {
    notifications: { agent_levels, show_bell: true },
    integrations: {
      telegram_enabled: true,
      telegram_allowed_users: [...ADMITTED],
      slack_enabled: true,
      slack_allowed_users: [...ADMITTED],
      discord_enabled: true,
      discord_allowed_users: [...ADMITTED],
    },
  } as unknown as AppConfig;
}

/** Every row also carries its own chime switch, which is not a destination
 *  column: the grid's switch count is the local surfaces plus that one. */
const SWITCHES_PER_ROW = LOCAL_DESTINATIONS.length + 1;

function switches(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="switch"]')) as HTMLButtonElement[];
}

function pickers(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll(PICKER)) as HTMLButtonElement[];
}

function pickerOptions(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="checkbox"]')) as HTMLButtonElement[];
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

async function openFirstPicker(host: Element): Promise<Element> {
  const row = host.querySelector(ROW);
  assert(row, "a level row is rendered");
  const trigger = row.querySelector(PICKER) as HTMLButtonElement | null;
  assert(trigger, "the level offers a push control");
  await act(async () => trigger.click());
  return row;
}

async function withDaemon(
  invoke: (command: string) => Promise<unknown>,
  body: () => Promise<void>,
) {
  __setInvokeForTest(async <T,>(command: string) => invoke(command) as Promise<T>);
  __setListenForTest(async () => () => {});
  try {
    await body();
  } finally {
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}

const cases: string[] = [];
async function check(name: string, run: () => Promise<void> | void) {
  await run();
  cases.push(name);
  console.log(`  ok ${cases.length} - ${name}`);
}

/** 1. A second channel widens one control rather than adding a row per level. */
async function aSecondChannelWidensTheControlWithNoEditHere() {
  const one = await mount(
    <NotificationDestinations config={configWith(ROWS)} channels={ONE_CHANNEL} onUpdate={() => {}} />,
  );
  const oneGrid = one.host.querySelector(GRID);
  assert(oneGrid, "the surface renders");
  assert(
    switches(oneGrid).length === ROWS.length * SWITCHES_PER_ROW,
    "the local surfaces are one toggle per level each",
  );
  assert(
    pickers(one.host).length === ROWS.length,
    `one push control per level, got ${pickers(one.host).length}`,
  );
  const oneRow = await openFirstPicker(one.host);
  assert(pickerOptions(oneRow).length === 1, "offering the single registered channel");
  await one.done();

  const two = await mount(
    <NotificationDestinations config={configWith(ROWS)} channels={TWO_CARRIERS} onUpdate={() => {}} />,
  );
  assert(
    pickers(two.host).length === ROWS.length,
    "a roster that gained a member did NOT gain a control per level",
  );
  const twoRow = await openFirstPicker(two.host);
  const named = pickerOptions(twoRow).map((control) => control.getAttribute("aria-label"));
  assert(
    JSON.stringify(named) ===
      JSON.stringify(TWO_CARRIERS.map((channel) => `${levelLabel("verbose")}: ${channel.label}`)),
    `it widened the one control instead, got ${JSON.stringify(named)}`,
  );
  await two.done();
}

/** 2. The system notification keeps its meaning, its place, and its own toggle,
 *  and the panel is offered as no column at all. */
async function theSystemNotificationKeepsItsOwnToggleAndField() {
  const columns = destinationColumns(TWO_CARRIERS);
  assert(
    JSON.stringify(columns.map((column) => column.id)) ===
      JSON.stringify(["desktop", ...TWO_CARRIERS.map((channel) => channel.id)]),
    `the local surface stays first, then the roster, got ${JSON.stringify(
      columns.map((column) => column.id),
    )}`,
  );
  assert(
    !columns.some((column) => column.id === ("bell" as string)),
    "and the panel is not among them: every record reaches it regardless",
  );

  const view = await mount(
    <NotificationDestinations config={configWith(ROWS)} channels={TWO_CARRIERS} onUpdate={() => {}} />,
  );
  for (const row of Array.from(view.host.querySelectorAll(ROW))) {
    const level = row.getAttribute("data-notification-level");
    const shipped = ROWS.find((candidate) => candidate.level === level);
    assert(shipped, `${level} came from the fixture`);
    const controls = switches(row);
    assert(
      controls.length === SWITCHES_PER_ROW,
      `${level}: the local surface and the chime are the switches on the row, got ${controls.length}`,
    );
    assert(
      controls[0].getAttribute("aria-checked") === String(shipped.desktop),
      `${level}: the desktop cell reads its own field`,
    );
    assert(
      !switches(row).some((control) => control.getAttribute("aria-label")?.includes("bell")),
      `${level}: nothing on the row asks about the bell`,
    );
  }
  await view.done();
}

/** 3. A row written before a channel existed reads that channel as closed. */
async function anAbsentChannelKeyReadsAsClosed() {
  for (const row of ROWS) {
    for (const channel of TWO_CARRIERS) {
      assert(
        !isRouted(row, channel.id),
        `${row.level} names no ${channel.id} key, so it does not reach it`,
      );
    }
  }
  assert(
    !reachesNothing(ROWS[0], destinationColumns(TWO_CARRIERS)),
    "a row with an open surface reaches something even with every channel unnamed",
  );
  const shut: AgentLevelChoice = { level: "verbose", bell: false, desktop: false };
  assert(
    reachesNothing(shut, destinationColumns([])),
    "with no channels registered, both surfaces closed is reaching nothing",
  );
  assert(
    reachesNothing(shut, destinationColumns(TWO_CARRIERS)),
    "an unnamed channel column is closed, so the level still reaches nothing",
  );
  // Named is what routes: picking the channel is the whole difference.
  assert(
    isRouted({ ...shut, slack: true }, "slack"),
    "the channel someone picked reaches its channel",
  );
}

/** 4. A channel the daemon cannot deliver through is offered nowhere. */
async function anUnavailableChannelIsOfferedNowhere() {
  assert(
    WITH_UNAVAILABLE.some((channel) => channel.id === "discord"),
    "the roster carries the unavailable channel, so its absence below is the filter",
  );
  assert(
    JSON.stringify(offerableChannels(WITH_UNAVAILABLE).map((channel) => channel.id)) ===
      JSON.stringify(["telegram"]),
    "and only one of the two can carry a message",
  );
  assert(
    pushDestinations(destinationColumns(WITH_UNAVAILABLE)).length === 1,
    "so it is not a push destination either",
  );

  const view = await mount(
    <PushChannelSettings channels={WITH_UNAVAILABLE} config={configWith(ROWS)} onUpdate={() => {}} />,
  );
  assert(
    view.host.querySelector(TELEGRAM_EDITOR),
    "an offerable channel with a real audience question gets its own settings surface",
  );
  assert(
    view.host.querySelector(DISCORD_EDITOR) === null,
    "a channel the daemon cannot offer draws no controls, however many ids it admits",
  );
  await view.done();

  const grid = await mount(
    <NotificationDestinations
      config={configWith(ROWS)}
      channels={WITH_UNAVAILABLE}
      onUpdate={() => {}}
    />,
  );
  const row = await openFirstPicker(grid.host);
  const named = pickerOptions(row).map((control) => control.getAttribute("aria-label"));
  assert(
    JSON.stringify(named) === JSON.stringify([`${levelLabel("verbose")}: Telegram`]),
    `and it is not one of the destinations offered, got ${JSON.stringify(named)}`,
  );
  await grid.done();
}

/** 5. A roster the daemon cannot answer for degrades to the local surfaces. */
async function anUnansweredRosterDrawsOnlyTheLocalSurfaces() {
  await withDaemon(
    async (command) => {
      if (command === "push_channel_roster") throw new Error("daemon unreachable");
      if (command === "list_agent_providers") return [];
      return null;
    },
    async () => {
      const view = await mount(
        <NotificationsOverview
          config={configWith(ROWS)}
          onUpdate={() => {}}
          onUpdateFullConfig={() => {}}
        />,
      );
      const grid = view.host.querySelector(GRID);
      assert(grid, "the surface still renders its local surfaces");
      assert(
        switches(grid).length === ROWS.length * SWITCHES_PER_ROW,
        "an unanswered roster draws no channel rather than guessing one",
      );
      assert(
        pickers(grid).length === 0,
        "and offers no push control at all, rather than an empty one",
      );
      assert(
        view.host.querySelector(TELEGRAM_EDITOR) === null,
        "and no channel's controls are offered",
      );
      await view.done();
    },
  );

  await withDaemon(
    async (command) => {
      if (command === "push_channel_roster") return { channels: TWO_CARRIERS };
      if (command === "list_agent_providers") return [];
      return null;
    },
    async () => {
      const view = await mount(
        <NotificationsOverview
          config={configWith(ROWS)}
          onUpdate={() => {}}
          onUpdateFullConfig={() => {}}
        />,
      );
      const grid = view.host.querySelector(GRID);
      assert(grid, "the surface renders");
      assert(
        switches(grid).length === ROWS.length * SWITCHES_PER_ROW &&
          pickers(grid).length === ROWS.length,
        "the answered roster is what the surface draws",
      );
      await view.done();
    },
  );
}

async function main() {
  await check(
    "channel_registry: a second channel widens the control with no edit here",
    aSecondChannelWidensTheControlWithNoEditHere,
  );
  await check(
    "channel_registry: the system notification keeps its own toggle and field",
    theSystemNotificationKeepsItsOwnToggleAndField,
  );
  await check(
    "channel_registry: an absent channel key reads as closed",
    anAbsentChannelKeyReadsAsClosed,
  );
  await check(
    "channel_registry: an unavailable channel is offered nowhere",
    anUnavailableChannelIsOfferedNowhere,
  );
  await check(
    "channel_registry: an unanswered roster draws only the local surfaces",
    anUnansweredRosterDrawsOnlyTheLocalSurfaces,
  );
  console.log(`PASS channel_registry — ${cases.length}/${cases.length} cases`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
