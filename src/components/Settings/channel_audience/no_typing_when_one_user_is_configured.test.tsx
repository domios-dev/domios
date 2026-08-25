// Run: npx tsx src/components/Settings/channel_audience/no_typing_when_one_user_is_configured.test.tsx
//
// The defect: the panel asked for what the user had already configured. With a
// single configured id, opening the channel's column for a level answers who
// as well, and nothing anywhere offers a place to type an id.

import { act } from "react";
import { NotificationDestinations } from "../NotificationDestinations";
import { PushChannelSettings } from "../PushChannelSettings";
import { channelRecipients, configuredUsers } from "../../../lib/pushChannels";
import { destinationCellLabel } from "../../../lib/notifyDestinations";
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
  pass,
  row,
  setupDom,
  switches,
  type UpdateCall,
} from "./harness";

setupDom();

const ONLY = ADMITTED[0];
const LEVEL = "failure";

function config() {
  return configWith({ admitted: [ONLY], levels: [row(LEVEL, false)] });
}

/** Opening the column stores the one configured id as the audience. */
async function openingTheColumnAnswersWhoWithoutTyping() {
  const start = config();
  assert(
    configuredUsers(start, CHANNEL.id).length === 1,
    "the fixture configures exactly one id, which is the unambiguous case",
  );
  assert(
    channelRecipients(start, CHANNEL.id).length === 0,
    "and no audience has been chosen yet, so the flip has something to answer",
  );

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
  const cell = named(view.host, destinationCellLabel(levelLabel(LEVEL), CHANNEL.label));
  await act(async () => cell.click());
  await view.done();

  assert(updates.length === 1, `one patch per flip, got ${updates.length}`);
  const after = applied(start, updates[0]);
  assert(
    JSON.stringify(channelRecipients(after, CHANNEL.id)) === JSON.stringify([ONLY]),
    `the stored audience is the configured id, got ${JSON.stringify(
      channelRecipients(after, CHANNEL.id),
    )}`,
  );

  const stored = after.notifications.channels?.[CHANNEL.id];
  assert(stored, "the channel's own entry was written");
  assert(
    JSON.stringify(Object.keys(stored).sort()) ===
      JSON.stringify(["per_scope_limit", "recipients"]),
    `the stored entry carries no second opt-in, got ${JSON.stringify(Object.keys(stored))}`,
  );
}

/** And there is nothing to type into: the surface offers the configured ids. */
async function theChannelSurfaceOffersRatherThanAsks() {
  const view = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={config()} onUpdate={() => {}} />,
  );
  const surface = view.host.querySelector('[data-testid="settings-channel-audience"]');
  assert(surface, "the offerable channel draws its audience surface");
  assert(
    surface.querySelectorAll("input, textarea").length === 0,
    "an id is chosen from what is configured, never typed in free text",
  );
  const offered = switches(surface).map((control) => control.getAttribute("aria-label"));
  assert(
    JSON.stringify(offered) === JSON.stringify([`${CHANNEL.label}: ${ONLY}`]),
    `the configured id is offered under a derived name, got ${JSON.stringify(offered)}`,
  );
  await view.done();
}

async function main() {
  await check(
    "channel_audience: opening the column answers who without typing",
    openingTheColumnAnswersWhoWithoutTyping,
  );
  await check(
    "channel_audience: the channel surface offers configured ids rather than asking",
    theChannelSurfaceOffersRatherThanAsks,
  );
  pass("channel_audience — no typing when one user is configured");
}

main().catch(fail);
