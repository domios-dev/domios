// The third section exists only where there is something to answer. That rule
// already existed and is `audienceIsAChoice`; what is asserted here is that the

import { PushChannelSettings } from "../PushChannelSettings";
import { audienceIsAChoice, configuredUsers } from "../../../lib/pushChannels";
import {
  ADMITTED,
  CHANNEL,
  SECTION,
  assert,
  check,
  configWith,
  fail,
  mount,
  pass,
  sectionName,
  setupDom,
  tiles,
  withDaemon,
} from "./harness";

setupDom();

const AUDIENCE = `Who ${CHANNEL.label} writes to`;
const GROUP = '[data-testid="settings-channel-audience"]';

async function open(admitted: readonly string[]) {
  const config = configWith({ admitted });
  return {
    config,
    view: await mount(
      <PushChannelSettings channels={[CHANNEL]} config={config} onUpdate={() => {}} />,
    ),
  };
}

/** 1. Several admitted ids is a real question, so the section is drawn, named,
 *  and holds one tile per id it offers. */
async function aRealChoiceGetsItsOwnSection() {
  await withDaemon(async () => {
    const { config, view } = await open(ADMITTED);
    assert(
      audienceIsAChoice(configuredUsers(config, CHANNEL.id)),
      "the fixture is the case where the audience is genuinely a choice",
    );
    const sections = Array.from(view.host.querySelectorAll(SECTION));
    assert(sections.length === 1, `one section for one channel, got ${sections.length}`);
    assert(
      sectionName(sections[0]) === AUDIENCE,
      `the section names the channel it is about, got "${sectionName(sections[0])}"`,
    );
    const group = view.host.querySelector(GROUP);
    assert(group, "the channel's recipients are drawn");
    assert(sections[0].contains(group), "and they live inside the section that announced them");
    assert(
      tiles(group).length === configuredUsers(config, CHANNEL.id).length,
      "one tile per offered id",
    );
    await view.done();
  });
}

/** 2. One admitted id is not a question. No section, and therefore no header
 *  announcing one. */
async function aSettledAudienceGetsNoSection() {
  await withDaemon(async () => {
    const settled = [ADMITTED[0]];
    const { config, view } = await open(settled);
    assert(
      !audienceIsAChoice(configuredUsers(config, CHANNEL.id)),
      "a single admitted id is the case the rule excludes",
    );
    assert(view.host.querySelector(GROUP) === null, "the audience surface is not drawn");
    assert(
      view.host.querySelectorAll(SECTION).length === 0,
      "and no header announces a question that is not being asked",
    );
    await view.done();
  });
}

/** 3. Nobody admitted is not a question either, and the channel is not offerable
 *  at all. */
async function anUnadmittedChannelGetsNoSection() {
  await withDaemon(async () => {
    const { config, view } = await open([]);
    assert(
      configuredUsers(config, CHANNEL.id).length === 0,
      "the fixture admits nobody to the bridge",
    );
    assert(view.host.querySelector(GROUP) === null, "so there is nothing to draw");
    assert(view.host.querySelectorAll(SECTION).length === 0, "and no section is announced");
    await view.done();
  });
}

async function main() {
  await check("settings_tiles: a real choice gets its own section", aRealChoiceGetsItsOwnSection);
  await check("settings_tiles: a settled audience gets no section", aSettledAudienceGetsNoSection);
  await check(
    "settings_tiles: an unadmitted channel gets no section",
    anUnadmittedChannelGetsNoSection,
  );
  pass("settings_tiles — the audience section is only a real choice");
}

main().catch(fail);
