// Run: npx tsx src/components/Settings/channel_audience/admission_list_is_never_written.test.tsx
//
// Two lists that look alike and must never merge. The admission list says who
// may talk to us; the audience says who we push to. This panel reads the first
// to offer options and writes only the second.

import { act } from "react";
import { NotificationDestinations } from "../NotificationDestinations";
import { PushChannelSettings } from "../PushChannelSettings";
import { configuredUsers, recipientLabel } from "../../../lib/pushChannels";
import { destinationCellLabel } from "../../../lib/notifyDestinations";
import { levelLabel } from "../../../lib/notifyLevels";
import {
  ADMITTED,
  CHANNEL,
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

const LEVEL = "verbose";

/** Every control this surface owns is flipped, and not one patch names an
 *  admission list or the section that holds it. */
async function nothingHereWritesTheAdmissionList() {
  const config = configWith({ chosen: [ADMITTED[1]], levels: [row(LEVEL, true)] });
  const updates: UpdateCall[] = [];
  const record = (section: string, patch: unknown) =>
    updates.push({ section, patch: patch as Record<string, unknown> });

  const audience = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={config} onUpdate={record} />,
  );
  const surface = audience.host.querySelector('[data-testid="settings-channel-audience"]');
  assert(surface, "the surface renders, so its silence means something");
  const offered = switches(surface);
  assert(offered.length === ADMITTED.length, "every configured id is a control here");
  for (const control of offered) {
    await act(async () => control.click());
  }
  await audience.done();

  const grid = await mount(
    <NotificationDestinations config={config} channels={[CHANNEL]} onUpdate={record} />,
  );
  await act(async () =>
    named(grid.host, destinationCellLabel(levelLabel(LEVEL), CHANNEL.label)).click(),
  );
  await grid.done();

  assert(updates.length === ADMITTED.length + 1, `every flip emitted one patch, got ${updates.length}`);
  for (const update of updates) {
    assert(
      update.section === "notifications",
      `this panel writes the notifications section alone, got ${update.section}`,
    );
    for (const key of Object.keys(update.patch)) {
      assert(
        !/_allowed_users$/.test(key),
        `a patch key named an admission list: ${key}`,
      );
    }
    assert(
      !JSON.stringify(update.patch).includes("allowed_users"),
      `an admission list appeared inside a patch: ${JSON.stringify(update.patch)}`,
    );
  }
}

/** The admission list is a source, and a source is read without being moved:
 *  the ids offered come from it while the list itself stays put. */
async function theAdmissionListIsReadAsASourceOfOptions() {
  const config = configWith({ levels: [row(LEVEL, true)] });
  const before = JSON.stringify(config.integrations.telegram_allowed_users);

  const view = await mount(
    <PushChannelSettings channels={[CHANNEL]} config={config} onUpdate={() => {}} />,
  );
  const offeredNames = switches(view.host).map((control) => control.getAttribute("aria-label"));
  assert(
    JSON.stringify(offeredNames) ===
      JSON.stringify(configuredUsers(config, CHANNEL.id).map((id) => recipientLabel(CHANNEL.label, id))),
    `the picker offers exactly the configured ids, got ${JSON.stringify(offeredNames)}`,
  );
  await view.done();

  assert(
    JSON.stringify(config.integrations.telegram_allowed_users) === before,
    "reading the source never rewrites it",
  );
  configuredUsers(config, CHANNEL.id).push("999999999");
  assert(
    JSON.stringify(config.integrations.telegram_allowed_users) === before,
    "and the offered list is a copy, so the surface cannot mutate the source",
  );
}

async function main() {
  await check(
    "channel_audience: nothing on this panel writes an admission list",
    nothingHereWritesTheAdmissionList,
  );
  await check(
    "channel_audience: the admission list is read as a source of options",
    theAdmissionListIsReadAsASourceOfOptions,
  );
  pass("channel_audience — the admission list is never written");
}

main().catch(fail);
