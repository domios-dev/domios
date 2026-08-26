// Run: npx tsx src/components/Settings/destination_picker/one_control_for_the_push_side.test.tsx
//
// THE DEFECT TEST. A level's push destination is ONE control that names
// channels, not a control per channel. Pre-change the grid drew a toggle per
// channel per level, so with two channels in the roster this file counted three
// push controls per level instead of one and failed on the first assertion.

import { NotificationDestinations } from "../NotificationDestinations";
import {
  NO_CHANNEL_SUMMARY,
  destinationSummary,
  isRouted,
  pushPickerLabel,
} from "../../../lib/notifyDestinations";
import { levelLabel } from "../../../lib/notifyLevels";
import {
  LEVELS,
  SLACK,
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
  pushTriggers,
  row,
  setupDom,
} from "./harness";

setupDom();

const ROSTER = [TELEGRAM, SLACK];
const ROWS = LEVELS.map((level) => row(level, { [TELEGRAM.id]: true }));

/** One push control per level, whatever the roster size. */
async function aLevelHasOnePushControlNotOnePerChannel() {
  const view = await mount(
    <NotificationDestinations
      config={configWith({ levels: ROWS })}
      channels={ROSTER}
      onUpdate={() => {}}
    />,
  );

  for (const level of LEVELS) {
    const scope = levelRow(view.host, level);
    const found = pushTriggers(scope);
    assert(
      found.length === 1,
      `${level} offers one push control for ${ROSTER.length} channels, got ${found.length}`,
    );
    assert(
      found[0].getAttribute("aria-label") === pushPickerLabel(levelLabel(level)),
      `${level}'s push control names the level it governs, got ${found[0].getAttribute(
        "aria-label",
      )}`,
    );
    // Nothing is offered until it is opened, which is what stops a roster that
    // grew from adding a row to every level.
    assert(
      options(scope).length === 0,
      `${level} draws no channel control while its picker is closed`,
    );
  }

  // The count does not scale with the roster: one control per level, total.
  assert(
    pushTriggers(view.host).length === ROWS.length,
    `the whole surface draws one push control per level, got ${pushTriggers(view.host).length}`,
  );
  await view.done();
}

/** Opened, it enumerates the roster — every channel, one control each, inside
 *  the one named group a reader can walk. */
async function theOpenedControlEnumeratesEveryChannelUnderOneName() {
  const view = await mount(
    <NotificationDestinations
      config={configWith({ levels: ROWS })}
      channels={ROSTER}
      onUpdate={() => {}}
    />,
  );
  const level = LEVELS[1];
  const trigger = await openPush(view.host, level);
  assert(trigger.getAttribute("aria-expanded") === "true", "the control reports itself open");

  const scope = levelRow(view.host, level);
  const group = scope.querySelector('[data-testid="settings-destination-picker-options"]');
  assert(group, "the opened control draws its options");
  assert(
    group.getAttribute("role") === "group" &&
      group.getAttribute("aria-label") === pushPickerLabel(levelLabel(level)),
    "and they sit inside one named group rather than loose in the row",
  );
  const offered = options(group).map((control) => control.getAttribute("aria-label"));
  assert(
    JSON.stringify(offered) ===
      JSON.stringify(ROSTER.map((channel) => `${levelLabel(level)}: ${channel.label}`)),
    `every channel is one enumerable option, got ${JSON.stringify(offered)}`,
  );
  assert(
    options(group).map((control) => control.getAttribute("aria-checked")).join(",") ===
      ROSTER.map((channel) => String(isRouted(ROWS[1], channel.id))).join(","),
    "and each reads back the state stored for it",
  );
  await view.done();
}

/** Selecting nothing is valid: the closed control says so and nothing refuses. */
async function aLevelMayReachNoChannelAtAll() {
  const none = LEVELS.map((level) => row(level));
  const view = await mount(
    <NotificationDestinations
      config={configWith({ levels: none })}
      channels={ROSTER}
      onUpdate={() => {}}
    />,
  );
  for (const level of LEVELS) {
    const scope = levelRow(view.host, level);
    const found = pushTriggers(scope);
    assert(found.length === 1, `${level} still offers its push control`);
    assert(
      found[0].textContent?.includes(NO_CHANNEL_SUMMARY),
      `${level} reads back as reaching no channel, got "${found[0].textContent}"`,
    );
  }
  await view.done();

  assert(destinationSummary([]) === NO_CHANNEL_SUMMARY, "no selection is a summary, not a blank");
  assert(
    destinationSummary([{ id: TELEGRAM.id, label: TELEGRAM.label, kind: "channel" }]) ===
      TELEGRAM.label,
    "and a selection reads back the captions the registry gave",
  );
}

/** An empty roster leaves the level its local surfaces and no empty control. */
async function anEmptyRosterDrawsNoPushControl() {
  const view = await mount(
    <NotificationDestinations config={configWith({ levels: ROWS })} channels={[]} onUpdate={() => {}} />,
  );
  assert(
    pushTriggers(view.host).length === 0,
    "with nothing registered there is nothing to pick, so no control is drawn",
  );
  assert(
    view.host.querySelectorAll('[data-testid="settings-notification-level"]').length ===
      ROWS.length,
    "while every level still renders its row",
  );
  await view.done();
}

async function main() {
  await check(
    "destination_picker: a level has one push control, not one per channel",
    aLevelHasOnePushControlNotOnePerChannel,
  );
  await check(
    "destination_picker: the opened control enumerates every channel under one name",
    theOpenedControlEnumeratesEveryChannelUnderOneName,
  );
  await check(
    "destination_picker: a level may reach no channel at all",
    aLevelMayReachNoChannelAtAll,
  );
  await check(
    "destination_picker: an empty roster draws no push control",
    anEmptyRosterDrawsNoPushControl,
  );
  pass("destination_picker — one control for the push side");
}

main().catch(fail);
