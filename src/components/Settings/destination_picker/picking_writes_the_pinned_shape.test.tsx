// The control changed; the stored shape did not. A channel is a flattened key
// that exists while it is picked and is GONE when it is not — a closed column is

import { act } from "react";
import { NotificationDestinations } from "../NotificationDestinations";
import {
  LOCAL_DESTINATIONS,
  destinationCellLabel,
  isRouted,
} from "../../../lib/notifyDestinations";
import { levelLabel } from "../../../lib/notifyLevels";
import type { AgentLevelChoice } from "../../../lib/types";
import {
  LEVELS,
  SLACK,
  TELEGRAM,
  applied,
  assert,
  check,
  configWith,
  fail,
  levelRow,
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

const ROSTER = [TELEGRAM, SLACK];
const LEVEL = LEVELS[1];
/** Two admitted ids per channel: enough that nothing is preselected, so the
 *  emitted patch is the row set alone and can be compared whole. */
const ADMITTED: Record<string, readonly string[]> = {
  [TELEGRAM.id]: ["100000001", "100000002"],
  [SLACK.id]: ["U100", "U200"],
};

function rows(): AgentLevelChoice[] {
  return LEVELS.map((level) => row(level));
}

function patchedRow(call: UpdateCall, level: string): AgentLevelChoice {
  const levels = call.patch.agent_levels as AgentLevelChoice[] | undefined;
  assert(levels, `the patch carries the row set, got ${JSON.stringify(call.patch)}`);
  const found = levels.find((candidate) => candidate.level === level);
  assert(found, `the patch still carries the ${level} row`);
  return found;
}

async function pick(config: ReturnType<typeof configWith>, level: string, channelLabel: string) {
  const updates: UpdateCall[] = [];
  const view = await mount(
    <NotificationDestinations
      config={config}
      channels={ROSTER}
      onUpdate={(section, patch) =>
        updates.push({ section, patch: patch as Record<string, unknown> })
      }
    />,
  );
  await openPush(view.host, level);
  await act(async () =>
    named(levelRow(view.host, level), destinationCellLabel(levelLabel(level), channelLabel)).click(),
  );
  await view.done();
  assert(updates.length === 1, `one patch per pick, got ${updates.length}`);
  return updates[0];
}

/** Picking sets the key; unpicking removes it. */
async function aChannelKeyExistsWhilePickedAndIsGoneWhenNot() {
  const start = configWith({ levels: rows(), admitted: ADMITTED });
  const opened = await pick(start, LEVEL, TELEGRAM.label);
  assert(
    JSON.stringify(opened.patch) ===
      JSON.stringify({
        agent_levels: LEVELS.map((level) =>
          level === LEVEL
            ? { level, bell: true, desktop: true, [TELEGRAM.id]: true }
            : { level, bell: true, desktop: true },
        ),
      }),
    `picking writes the key and nothing else, got ${JSON.stringify(opened.patch)}`,
  );

  const after = applied(start, opened);
  const closed = await pick(after, LEVEL, TELEGRAM.label);
  const shut = patchedRow(closed, LEVEL);
  assert(
    !(TELEGRAM.id in shut),
    `unpicking removes the key rather than storing false, got ${JSON.stringify(shut)}`,
  );
  assert(
    JSON.stringify(shut) === JSON.stringify({ level: LEVEL, bell: true, desktop: true }),
    `and leaves the row exactly as it was before, got ${JSON.stringify(shut)}`,
  );
  assert(!isRouted(shut, TELEGRAM.id), "which reads back closed, as an absent key always has");
}

/** Several channels at once: each key stands on its own. */
async function eachChannelKeyStandsOnItsOwn() {
  const start = configWith({ levels: rows(), admitted: ADMITTED });
  const first = await pick(start, LEVEL, TELEGRAM.label);
  const both = await pick(applied(start, first), LEVEL, SLACK.label);
  const picked = patchedRow(both, LEVEL);
  assert(
    picked[TELEGRAM.id] === true && picked[SLACK.id] === true,
    `both picked channels are keys on the row, got ${JSON.stringify(picked)}`,
  );

  const dropped = patchedRow(
    await pick(applied(applied(start, first), both), LEVEL, TELEGRAM.label),
    LEVEL,
  );
  assert(
    !(TELEGRAM.id in dropped) && dropped[SLACK.id] === true,
    `dropping one leaves the other untouched, got ${JSON.stringify(dropped)}`,
  );
}

/** A level nobody touched is not rewritten by a pick on another level. */
async function noNeighbouringRowMoves() {
  const start = configWith({ levels: rows(), admitted: ADMITTED });
  const call = await pick(start, LEVEL, SLACK.label);
  for (const level of LEVELS) {
    if (level === LEVEL) continue;
    assert(
      JSON.stringify(patchedRow(call, level)) ===
        JSON.stringify({ level, bell: true, desktop: true }),
      `${level} was not touched, got ${JSON.stringify(patchedRow(call, level))}`,
    );
  }
}

/** The local surface is unchanged: its own toggle, its own named field, and an
 *  explicit answer in both directions rather than a vanishing key. The retained
 *  `bell` field rides along untouched, because nothing on this surface writes it. */
async function theLocalSurfacesKeepTheirOwnTogglesAndFields() {
  const start = configWith({ levels: rows(), admitted: ADMITTED });
  for (const surface of LOCAL_DESTINATIONS) {
    const updates: UpdateCall[] = [];
    const view = await mount(
      <NotificationDestinations
        config={start}
        channels={ROSTER}
        onUpdate={(section, patch) =>
          updates.push({ section, patch: patch as Record<string, unknown> })
        }
      />,
    );
    const scope = levelRow(view.host, LEVEL);
    // The chime is a switch on the row too, and is not one of the destinations.
    assert(
      switches(scope).length === LOCAL_DESTINATIONS.length + 1,
      `each local surface keeps its own switch beside the chime, got ${switches(scope).length}`,
    );
    const control = named(scope, destinationCellLabel(levelLabel(LEVEL), surface.label));
    assert(
      control.getAttribute("role") === "switch",
      `${surface.id} is still a switch, not an option in the picker`,
    );
    await act(async () => control.click());
    await view.done();

    assert(updates.length === 1, `one patch per flip, got ${updates.length}`);
    const flipped = patchedRow(updates[0], LEVEL);
    assert(
      surface.id in flipped && flipped[surface.id] === false,
      `${surface.id} keeps an explicit answer rather than losing its key, got ${JSON.stringify(
        flipped,
      )}`,
    );
    assert(
      JSON.stringify(flipped) ===
        JSON.stringify({ level: LEVEL, bell: true, desktop: surface.id !== "desktop" }),
      `only the flipped surface moved, got ${JSON.stringify(flipped)}`,
    );
    assert(
      flipped.bell === true,
      "and the retained bell field, which this surface never offers, is carried through",
    );
  }
}

async function main() {
  await check(
    "destination_picker: a channel key exists while picked and is gone when not",
    aChannelKeyExistsWhilePickedAndIsGoneWhenNot,
  );
  await check(
    "destination_picker: each channel key stands on its own",
    eachChannelKeyStandsOnItsOwn,
  );
  await check("destination_picker: no neighbouring row moves", noNeighbouringRowMoves);
  await check(
    "destination_picker: the local surfaces keep their own toggles and fields",
    theLocalSurfacesKeepTheirOwnTogglesAndFields,
  );
  pass("destination_picker — picking writes the pinned shape");
}

main().catch(fail);
