// A channel that cannot carry a message must not be offered as a destination.
// The daemon already answers this per roster entry; nothing read it. Every

import { NotificationDestinations } from "../NotificationDestinations";
import { PushChannelSettings } from "../PushChannelSettings";
import {
  destinationColumns,
  offerableChannels,
  pushDestinations,
} from "../../../lib/notifyDestinations";
import { levelLabel } from "../../../lib/notifyLevels";
import {
  DISCORD,
  LEVELS,
  TELEGRAM,
  assert,
  check,
  configWith,
  fail,
  levelRow,
  mount,
  openPush,
  options,
  pass,
  row,
  setupDom,
} from "./harness";

setupDom();

/** Listed by the daemon, and one of the two cannot carry anything. */
const ROSTER = [TELEGRAM, DISCORD];
const ROWS = LEVELS.map((level) => row(level));
const LEVEL = LEVELS[0];

/** The opened control offers the carrier and not the one with no transport. */
async function anUnavailableChannelIsNotAnOption() {
  assert(
    ROSTER.some((channel) => channel.id === DISCORD.id),
    "the fixture keeps the unavailable channel in the roster, so its absence below means something",
  );
  assert(
    ROSTER.filter((channel) => channel.control_available).length === 1,
    "and exactly one of the two can carry a message",
  );

  const view = await mount(
    <NotificationDestinations
      config={configWith({ levels: ROWS })}
      channels={ROSTER}
      onUpdate={() => {}}
    />,
  );
  await openPush(view.host, LEVEL);
  const offered = options(levelRow(view.host, LEVEL)).map((control) =>
    control.getAttribute("aria-label"),
  );
  assert(
    JSON.stringify(offered) === JSON.stringify([`${levelLabel(LEVEL)}: ${TELEGRAM.label}`]),
    `only the channel that can carry is offered, got ${JSON.stringify(offered)}`,
  );
  assert(
    !offered.some((name) => name?.includes(DISCORD.label)),
    `${DISCORD.label} has no transport in this build and must not be offered`,
  );
  await view.done();
}

/** The column set itself refuses it, so no surface built on it can offer it. */
async function theColumnSetItselfRefusesIt() {
  const columns = destinationColumns(ROSTER);
  assert(
    JSON.stringify(pushDestinations(columns).map((column) => column.id)) ===
      JSON.stringify([TELEGRAM.id]),
    `an unavailable channel is not a push column, got ${JSON.stringify(
      pushDestinations(columns).map((column) => column.id),
    )}`,
  );
  assert(
    JSON.stringify(offerableChannels(ROSTER).map((channel) => channel.id)) ===
      JSON.stringify([TELEGRAM.id]),
    "and the roster filter names the same one",
  );
  assert(
    offerableChannels([DISCORD]).length === 0,
    "a roster of nothing but unavailable channels offers nothing",
  );
  assert(
    offerableChannels([]).length === 0,
    "and an empty roster offers nothing rather than guessing a member",
  );
}

/** Nor does it get an audience surface, with the same roster in hand. */
async function anUnavailableChannelDrawsNoAudienceSurface() {
  const config = configWith({
    levels: ROWS,
    admitted: { [TELEGRAM.id]: ["100000001", "100000002"], [DISCORD.id]: ["9001", "9002"] },
  });
  const view = await mount(
    <PushChannelSettings channels={ROSTER} config={config} onUpdate={() => {}} />,
  );
  assert(
    view.host.querySelector(
      `[data-testid="settings-channel-audience"][data-channel="${TELEGRAM.id}"]`,
    ),
    "the channel that can carry gets its audience surface, so the absence below is the filter",
  );
  assert(
    view.host.querySelector(
      `[data-testid="settings-channel-audience"][data-channel="${DISCORD.id}"]`,
    ) === null,
    "and the one that cannot draws nothing, however many ids it admits",
  );
  await view.done();
}

async function main() {
  await check(
    "destination_picker: an unavailable channel is not an option",
    anUnavailableChannelIsNotAnOption,
  );
  await check(
    "destination_picker: the column set itself refuses it",
    theColumnSetItselfRefusesIt,
  );
  await check(
    "destination_picker: an unavailable channel draws no audience surface",
    anUnavailableChannelDrawsNoAudienceSurface,
  );
  pass("destination_picker — an unavailable channel is not offered");
}

main().catch(fail);
