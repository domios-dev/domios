// Assert both directions explicitly so behavior does not depend on an unnamed column default.

import { act } from "react";
import { NotificationDestinations } from "../NotificationDestinations";
import { PushChannelSettings } from "../PushChannelSettings";
import { channelRecipients, recipientLabel } from "../../../lib/pushChannels";
import { destinationCellLabel, isRouted } from "../../../lib/notifyDestinations";
import { levelLabel } from "../../../lib/notifyLevels";
import {
  ADMITTED,
  CHANNEL,
  applied,
  assert,
  check,
  configWith,
  fail,
  mount,
  named,
  openPush,
  pass,
  row,
  setupDom,
  switches,
  type AdmittedId,
  type UpdateCall,
} from "./harness";

setupDom();

const CHOSEN: readonly AdmittedId[] = [ADMITTED[0]];
const LEVEL = "failure";

/** Closed reaches nothing, open reaches the selection — both stated, neither
 *  inherited, with the same audience configured under each. */
async function theColumnDecidesWhetherAndTheAudienceDecidesWho() {
  const closed = configWith({ chosen: CHOSEN, levels: [row(LEVEL, false)] });
  const open = configWith({ chosen: CHOSEN, levels: [row(LEVEL, true)] });

  assert(
    isRouted(closed.notifications.agent_levels[0], CHANNEL.id) === false,
    "the closed fixture states the column closed rather than leaving it unnamed",
  );
  assert(
    isRouted(open.notifications.agent_levels[0], CHANNEL.id) === true,
    "and the open fixture states it open",
  );
  assert(
    JSON.stringify(channelRecipients(closed, CHANNEL.id)) === JSON.stringify(CHOSEN) &&
      JSON.stringify(channelRecipients(open, CHANNEL.id)) === JSON.stringify(CHOSEN),
    "the audience is identical under both, so the column is the only difference",
  );

  for (const [label, config, routed] of [
    ["closed", closed, false],
    ["open", open, true],
  ] as const) {
    const view = await mount(
      <NotificationDestinations config={config} channels={[CHANNEL]} onUpdate={() => {}} />,
    );
    await openPush(view.host, LEVEL);
    assert(
      named(view.host, destinationCellLabel(levelLabel(LEVEL), CHANNEL.label)).getAttribute(
        "aria-checked",
      ) === String(routed),
      `the ${label} column reads back what was stored for it`,
    );
    await view.done();
  }
}

/** Closing the column does not throw the audience away: whether and who are
 *  separate answers, so reopening reaches the same people. */
async function closingTheColumnLeavesTheAudienceIntact() {
  const start = configWith({ chosen: CHOSEN, levels: [row(LEVEL, true)] });
  const updates: UpdateCall[] = [];
  const view = await mount(
    <NotificationDestinations
      config={start}
      channels={[CHANNEL]}
      onUpdate={(section, patch) =>
        updates.push({ section, patch: patch as Record<string, unknown> })
      }
    />,
  );
  await openPush(view.host, LEVEL);
  await act(async () =>
    named(view.host, destinationCellLabel(levelLabel(LEVEL), CHANNEL.label)).click(),
  );
  await view.done();

  assert(updates.length === 1, `one patch per flip, got ${updates.length}`);
  const after = applied(start, updates[0]);
  assert(
    isRouted(after.notifications.agent_levels[0], CHANNEL.id) === false,
    "the flip closed the column",
  );
  assert(
    JSON.stringify(channelRecipients(after, CHANNEL.id)) === JSON.stringify(CHOSEN),
    `and left the audience alone, got ${JSON.stringify(channelRecipients(after, CHANNEL.id))}`,
  );
}

/** There is no second whether: the channel's surface offers one control per
 *  configured id and nothing that governs the channel as a whole. */
async function theChannelSurfaceCarriesNoSecondOptIn() {
  const config = configWith({ chosen: CHOSEN, levels: [row(LEVEL, true)] });
  const view = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={config} onUpdate={() => {}} />,
  );
  const surface = view.host.querySelector('[data-testid="settings-channel-audience"]');
  assert(surface, "the surface renders");

  const names = switches(surface).map((control) => control.getAttribute("aria-label"));
  assert(
    JSON.stringify(names) ===
      JSON.stringify(ADMITTED.map((id) => recipientLabel(CHANNEL.label, id))),
    `every control names one configured id and no more, got ${JSON.stringify(names)}`,
  );
  await view.done();
}

async function main() {
  await check(
    "channel_audience: the column decides whether and the audience decides who",
    theColumnDecidesWhetherAndTheAudienceDecidesWho,
  );
  await check(
    "channel_audience: closing the column leaves the audience intact",
    closingTheColumnLeavesTheAudienceIntact,
  );
  await check(
    "channel_audience: the channel surface carries no second opt-in",
    theChannelSurfaceCarriesNoSecondOptIn,
  );
  pass("channel_audience — the column is the only whether");
}

main().catch(fail);
