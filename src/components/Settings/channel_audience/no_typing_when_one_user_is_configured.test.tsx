// Run: npx tsx src/components/Settings/channel_audience/no_typing_when_one_user_is_configured.test.tsx
//
// The defect: the panel asked for what the user had already configured. With a
// single admitted id there is nothing to ask — no surface at all — and picking
// the channel for a level answers who as well. Both halves are asserted
// together, because the absence of a widget is worthless without the stored id.

import { act } from "react";
import { NotificationDestinations } from "../NotificationDestinations";
import { PushChannelSettings } from "../PushChannelSettings";
import { audienceIsAChoice, channelRecipients, configuredUsers } from "../../../lib/pushChannels";
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
  openPush,
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

/** No surface, and the id stored anyway. */
async function oneAdmittedIdIsNeverAskedAboutAndIsStillStored() {
  const start = config();
  assert(
    configuredUsers(start, CHANNEL.id).length === 1,
    "the fixture admits exactly one id, which is the unambiguous case",
  );
  assert(
    !audienceIsAChoice(configuredUsers(start, CHANNEL.id)),
    "and one admitted id is not a choice to put a surface on",
  );
  assert(
    channelRecipients(start, CHANNEL.id).length === 0,
    "no audience has been chosen yet, so picking the channel has something to answer",
  );

  // Half one: nothing asks. The channel IS offerable — control_available is
  // true and the roster carries it — so an empty render here is the rule, not
  // an unrelated absence.
  const surface = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={start} onUpdate={() => {}} />,
  );
  assert(
    surface.host.querySelector('[data-testid="settings-channel-audience"]') === null,
    "a set of one is not a question, so no audience surface is drawn",
  );
  assert(
    surface.host.querySelectorAll("input, textarea").length === 0,
    "and there is nothing to type an id into either",
  );
  assert(
    switches(surface.host).length === 0,
    "nor a toggle asking the user to confirm the one id they already configured",
  );
  await surface.done();

  // Half two: picking the channel stores that id as the audience.
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

  assert(updates.length === 1, `one patch per pick, got ${updates.length}`);
  const after = applied(start, updates[0]);
  assert(
    JSON.stringify(channelRecipients(after, CHANNEL.id)) === JSON.stringify([ONLY]),
    `the stored audience is the admitted id, got ${JSON.stringify(
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

/** The rule is about there being a choice, not about hiding the surface: two
 *  admitted ids and it is back, offering both. */
async function severalAdmittedIdsStillGetTheSurface() {
  const several = configWith({
    admitted: [ADMITTED[0], ADMITTED[1]],
    levels: [row(LEVEL, true)],
  });
  assert(
    audienceIsAChoice(configuredUsers(several, CHANNEL.id)),
    "two admitted ids is a genuine choice",
  );
  const view = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={several} onUpdate={() => {}} />,
  );
  const surface = view.host.querySelector('[data-testid="settings-channel-audience"]');
  assert(surface, "so the surface is drawn");
  assert(
    switches(surface).length === 2,
    `one control per admitted id, got ${switches(surface).length}`,
  );
  await view.done();
}

async function main() {
  await check(
    "channel_audience: one admitted id is never asked about and is stored anyway",
    oneAdmittedIdIsNeverAskedAboutAndIsStillStored,
  );
  await check(
    "channel_audience: several admitted ids still get the surface",
    severalAdmittedIdsStillGetTheSurface,
  );
  pass("channel_audience — no typing when one user is configured");
}

main().catch(fail);
