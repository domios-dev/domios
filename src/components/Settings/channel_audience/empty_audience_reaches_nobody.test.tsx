// An empty selection must mean nobody even when the admission list is populated.

import { act } from "react";
import { PushChannelSettings } from "../PushChannelSettings";
import { channelRecipients, configuredUsers, recipientLabel } from "../../../lib/pushChannels";
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

const LEVELS = [row("verbose", true), row("action_needed", true), row("failure", true)];

/** Deselecting the last chosen id stores an empty audience — and the emptiness
 *  is answered against an admission list that is deliberately not empty. */
async function clearingTheSelectionStoresNobody() {
  const start = configWith({ chosen: [ADMITTED[0]], levels: LEVELS });
  const updates: UpdateCall[] = [];
  const view = await mount(
    <PushChannelSettings
      channels={[CHANNEL]}
      config={start}
      onUpdate={(section, patch) =>
        updates.push({ section, patch: patch as Record<string, unknown> })
      }
    />,
  );
  await act(async () => named(view.host, recipientLabel(CHANNEL.label, ADMITTED[0])).click());
  await view.done();

  assert(updates.length === 1, `one patch per flip, got ${updates.length}`);
  const after = applied(start, updates[0]);

  assert(
    configuredUsers(after, CHANNEL.id).length === ADMITTED.length,
    "the admission list is untouched and still populated, so empty is a choice",
  );
  assert(
    channelRecipients(after, CHANNEL.id).length === 0,
    `the audience is empty, got ${JSON.stringify(channelRecipients(after, CHANNEL.id))}`,
  );

  // Check each admitted id so falling back to the admission list cannot pass as an empty audience.
  for (const admitted of configuredUsers(after, CHANNEL.id)) {
    assert(
      !channelRecipients(after, CHANNEL.id).includes(admitted),
      `${admitted} is admitted inbound and must not become a push recipient`,
    );
  }
  assert(
    JSON.stringify(channelRecipients(after, CHANNEL.id)) !==
      JSON.stringify(configuredUsers(after, CHANNEL.id)),
    "an empty audience never resolves to the admission list",
  );
}

/** A channel nobody was chosen for offers every configured id UNCHECKED, so the
 *  surface cannot imply an audience the stored config does not hold. */
async function anUnchosenChannelShowsNobodySelected() {
  const config = configWith({ levels: LEVELS });
  assert(
    configuredUsers(config, CHANNEL.id).length === ADMITTED.length,
    "ids are admitted inbound",
  );
  assert(channelRecipients(config, CHANNEL.id).length === 0, "and none is an audience");

  const view = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={config} onUpdate={() => {}} />,
  );
  const surface = view.host.querySelector('[data-testid="settings-channel-audience"]');
  assert(surface, "the surface renders");
  const offered = switches(surface);
  assert(
    offered.length === ADMITTED.length,
    `every configured id is offered, got ${offered.length}`,
  );
  assert(
    offered.every((control) => control.getAttribute("aria-checked") === "false"),
    "no configured id reads as selected while the stored audience is empty",
  );
  await view.done();
}

async function main() {
  await check(
    "channel_audience: clearing the selection stores nobody, with no admission fallback",
    clearingTheSelectionStoresNobody,
  );
  await check(
    "channel_audience: an unchosen channel shows nobody selected",
    anUnchosenChannelShowsNobodySelected,
  );
  pass("channel_audience — an empty audience reaches nobody");
}

main().catch(fail);
