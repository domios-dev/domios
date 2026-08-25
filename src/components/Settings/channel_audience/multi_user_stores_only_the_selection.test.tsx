// Run: npx tsx src/components/Settings/channel_audience/multi_user_stores_only_the_selection.test.tsx
//
// Several configured ids is the real case. Opening the column preselects
// nothing, and what is stored afterwards is exactly what was picked.

import { act } from "react";
import { NotificationDestinations } from "../NotificationDestinations";
import { PushChannelSettings } from "../PushChannelSettings";
import {
  channelRecipients,
  configuredUsers,
  preselectedAudience,
  recipientLabel,
} from "../../../lib/pushChannels";
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
  type AdmittedId,
  type UpdateCall,
} from "./harness";

setupDom();

const LEVEL = "action_needed";

/** With more than one configured id, opening the column answers whether and
 *  leaves who to the user. Preselecting all of them would make one flip a
 *  broadcast to every admitted id. */
async function openingTheColumnPreselectsNobodyWhenSeveralAreConfigured() {
  assert(
    JSON.stringify(preselectedAudience(ADMITTED)) === "[]",
    "several configured ids preselect nobody",
  );
  assert(
    JSON.stringify(preselectedAudience([ADMITTED[1]])) === JSON.stringify([ADMITTED[1]]),
    "and exactly one configured id is the only unambiguous answer",
  );

  const start = configWith({ levels: [row(LEVEL, false)] });
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
  await act(async () =>
    named(view.host, destinationCellLabel(levelLabel(LEVEL), CHANNEL.label)).click(),
  );
  await view.done();

  assert(updates.length === 1, `one patch per flip, got ${updates.length}`);
  const after = applied(start, updates[0]);
  assert(
    channelRecipients(after, CHANNEL.id).length === 0,
    `nobody is preselected, got ${JSON.stringify(channelRecipients(after, CHANNEL.id))}`,
  );
  assert(
    configuredUsers(after, CHANNEL.id).length === ADMITTED.length,
    "while every id stays configured and therefore offerable",
  );
}

/** Picking two of three stores those two, in the order they were offered. */
async function theStoredAudienceIsExactlyWhatWasPicked() {
  let config = configWith({ levels: [row(LEVEL, true)] });
  const picked: readonly AdmittedId[] = [ADMITTED[0], ADMITTED[2]];

  for (const id of picked) {
    const updates: UpdateCall[] = [];
    const current = config;
    const view = await mount(
      <PushChannelSettings
        channels={[CHANNEL]}
        config={current}
        onUpdate={(section, patch) =>
          updates.push({ section, patch: patch as Record<string, unknown> })
        }
      />,
    );
    await act(async () => named(view.host, recipientLabel(CHANNEL.label, id)).click());
    await view.done();
    assert(updates.length === 1, `one patch per pick, got ${updates.length}`);
    config = applied(current, updates[0]);
  }

  assert(
    JSON.stringify(channelRecipients(config, CHANNEL.id)) === JSON.stringify(picked),
    `the stored audience is the picked ids, got ${JSON.stringify(
      channelRecipients(config, CHANNEL.id),
    )}`,
  );
  assert(
    !channelRecipients(config, CHANNEL.id).includes(ADMITTED[1]),
    `${ADMITTED[1]} was admitted but never picked, so it is not an audience`,
  );

  // Every offered id now reads back exactly its stored state.
  const view = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={config} onUpdate={() => {}} />,
  );
  for (const id of ADMITTED) {
    assert(
      named(view.host, recipientLabel(CHANNEL.label, id)).getAttribute("aria-checked") ===
        String(picked.includes(id)),
      `${id} reads back the selection that was stored for it`,
    );
  }
  await view.done();
}

async function main() {
  await check(
    "channel_audience: several configured ids preselect nobody",
    openingTheColumnPreselectsNobodyWhenSeveralAreConfigured,
  );
  await check(
    "channel_audience: the stored audience is exactly what was picked",
    theStoredAudienceIsExactlyWhatWasPicked,
  );
  pass("channel_audience — multi-user stores only the selection");
}

main().catch(fail);
