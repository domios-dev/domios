// Run: npx tsx src/components/Settings/channel_registry.test.tsx
//
// The grid side of the push-channel registry: columns come from the daemon's
// roster, so a channel it gains becomes a column with nothing here to edit.

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
const TELEGRAM_EDITOR = '[data-testid="settings-telegram-notifications"]';

const ONE_CHANNEL: PushChannelDescriptor[] = [
  { id: "telegram", label: "Telegram", control_available: true },
];

/** The second member, exactly as the daemon reports it: registry data only. */
const TWO_CHANNELS: PushChannelDescriptor[] = [
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

function configWith(agent_levels: AgentLevelChoice[]): AppConfig {
  return { notifications: { agent_levels, show_bell: true } } as unknown as AppConfig;
}

type UpdateCall = { section: string; patch: Record<string, unknown> };

function switches(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="switch"]')) as HTMLButtonElement[];
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

/** 1. A second channel becomes a column because the roster carries it. */
async function aSecondChannelBecomesAColumnWithNoEditHere() {
  const updates: UpdateCall[] = [];
  const one = await mount(
    <NotificationDestinations
      config={configWith(ROWS)}
      channels={ONE_CHANNEL}
      onUpdate={(section, patch) =>
        updates.push({ section, patch: patch as Record<string, unknown> })
      }
    />,
  );
  const before = switches(one.host.querySelector(GRID)!).length;
  assert(
    before === ROWS.length * (LOCAL_DESTINATIONS.length + ONE_CHANNEL.length),
    `columns are the surfaces plus the roster, got ${before}`,
  );
  await one.done();

  const two = await mount(
    <NotificationDestinations
      config={configWith(ROWS)}
      channels={TWO_CHANNELS}
      onUpdate={() => {}}
    />,
  );
  const grid = two.host.querySelector(GRID);
  assert(grid, "the grid renders");
  assert(
    switches(grid).length === ROWS.length * (LOCAL_DESTINATIONS.length + TWO_CHANNELS.length),
    "a roster that gained a member gained a column per row",
  );
  const named = switches(grid).map((control) => control.getAttribute("aria-label"));
  assert(
    named.includes(`${levelLabel("verbose")}: Discord`),
    `the new column carries the caption the registry gave it, got ${JSON.stringify(named)}`,
  );
  await two.done();
}

/** 2. Bell and desktop keep their meaning and their order. */
async function bellAndDesktopStayTheFirstTwoLocalSurfaces() {
  const columns = destinationColumns(TWO_CHANNELS);
  assert(
    JSON.stringify(columns.slice(0, 2).map((column) => column.id)) ===
      JSON.stringify(["bell", "desktop"]),
    "the local surfaces stay first and unchanged",
  );

  const view = await mount(
    <NotificationDestinations config={configWith(ROWS)} channels={TWO_CHANNELS} onUpdate={() => {}} />,
  );
  for (const row of Array.from(view.host.querySelectorAll(ROW))) {
    const level = row.getAttribute("data-notification-level");
    const shipped = ROWS.find((candidate) => candidate.level === level);
    assert(shipped, `${level} came from the fixture`);
    const controls = switches(row);
    assert(
      controls[0].getAttribute("aria-checked") === String(shipped.bell),
      `${level}: the bell cell reads its own field`,
    );
    assert(
      controls[1].getAttribute("aria-checked") === String(shipped.desktop),
      `${level}: the desktop cell reads its own field`,
    );
  }
  await view.done();
}

/** 3. A row written before a channel existed still reaches it. */
async function anAbsentChannelKeyReadsAsClosed() {
  for (const row of ROWS) {
    for (const channel of TWO_CHANNELS) {
      assert(
        !isRouted(row, channel.id),
        `${row.level} names no ${channel.id} key, so it does not reach it`,
      );
    }
  }
  assert(
    !reachesNothing(ROWS[0], destinationColumns(TWO_CHANNELS)),
    "a row with an open surface reaches something even with every channel unnamed",
  );
  const shut: AgentLevelChoice = { level: "verbose", bell: false, desktop: false };
  assert(
    reachesNothing(shut, destinationColumns([])),
    "with no channels registered, both surfaces closed is reaching nothing",
  );
  assert(
    reachesNothing(shut, destinationColumns(TWO_CHANNELS)),
    "an unnamed channel column is closed, so the level still reaches nothing",
  );
  // Named is what routes: opting the column in is the whole difference.
  assert(
    isRouted({ ...shut, discord: true }, "discord"),
    "the column someone switched on reaches its channel",
  );
}

/** 4. An unavailable channel is still routable, but offers no controls. */
async function anUnavailableChannelIsAColumnWithoutControls() {
  const view = await mount(
    <PushChannelSettings channels={TWO_CHANNELS} config={configWith(ROWS)} onUpdate={() => {}} />,
  );
  assert(
    view.host.querySelector(TELEGRAM_EDITOR),
    "an offerable channel gets its own settings surface",
  );
  await view.done();

  const none = await mount(
    <PushChannelSettings
      channels={TWO_CHANNELS.map((channel) => ({ ...channel, control_available: false }))}
      config={configWith(ROWS)}
      onUpdate={() => {}}
    />,
  );
  assert(
    none.host.querySelector(TELEGRAM_EDITOR) === null,
    "a channel the daemon cannot offer draws no controls",
  );
  await none.done();

  // The column itself survives: routing to an unavailable channel stays
  // expressible, which is what makes the daemon's refusal visible later.
  const grid = await mount(
    <NotificationDestinations config={configWith(ROWS)} channels={TWO_CHANNELS} onUpdate={() => {}} />,
  );
  const named = switches(grid.host.querySelector(GRID)!).map((c) => c.getAttribute("aria-label"));
  assert(
    named.includes(`${levelLabel("failure")}: Discord`),
    "an unavailable channel is still a column of the grid",
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
      assert(grid, "the grid still renders its local surfaces");
      assert(
        switches(grid).length === ROWS.length * LOCAL_DESTINATIONS.length,
        "an unanswered roster draws no channel column rather than guessing one",
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
      if (command === "push_channel_roster") return { channels: TWO_CHANNELS };
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
      assert(grid, "the grid renders");
      assert(
        switches(grid).length === ROWS.length * (LOCAL_DESTINATIONS.length + TWO_CHANNELS.length),
        "the answered roster is what the grid draws",
      );
      await view.done();
    },
  );
}

async function main() {
  await check(
    "channel_registry: a second channel becomes a column with no edit here",
    aSecondChannelBecomesAColumnWithNoEditHere,
  );
  await check(
    "channel_registry: bell and desktop stay the first two local surfaces",
    bellAndDesktopStayTheFirstTwoLocalSurfaces,
  );
  await check(
    "channel_registry: an absent channel key reads as closed",
    anAbsentChannelKeyReadsAsClosed,
  );
  await check(
    "channel_registry: an unavailable channel is a column without controls",
    anUnavailableChannelIsAColumnWithoutControls,
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
