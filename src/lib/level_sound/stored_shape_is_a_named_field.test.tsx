/**
 * Run: npm run test:unit -- level_sound
 *
 * The stored shape of the per-level chime.
 *
 * `channels` is FLATTENED onto the row, so any new key that is not a NAMED
 * field deserialises into that map as a push channel with that name. The
 * retained `bell` field exists for exactly this reason, and a previous slice was
 * bitten by it in reverse. `sound` is therefore a named field on both sides of
 * the wire, and nothing that writes a row may reach it through the channel path.
 *
 * The serde round trip itself is the daemon's claim and is pinned in
 * `codeterm-types` (`notification_level_sound_round_trips_and_never_lands_in_the
 * _channel_map`); this file pins the two sides agreeing on the shape and the
 * writers on this side staying out of the flattened map.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { destinationColumns, setLevelDestination } from "../notifyDestinations";
import { levelSounds, setLevelSound } from "../notificationSound";
import type { AgentLevelChoice } from "../types";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  if (!condition) throw new Error(message);
}

const here = dirname(fileURLToPath(import.meta.url));
const read = (relative: string) => readFileSync(resolve(here, relative), "utf8");

// 1. A named field on the daemon side, declared BEFORE the flattened map so the
//    map cannot claim the key, and optional so an unanswered row stays unanswered.
const schema = read("../../../src-tauri/crates/codeterm-types/src/config/schema.rs");
const row = schema.slice(schema.indexOf("pub struct AgentLevelChoice"));
const body = row.slice(0, row.indexOf("\n}"));
check(/pub sound: Option<bool>,/.test(body), "the row names the chime as its own field");
check(
  body.indexOf("pub sound:") < body.indexOf("#[serde(flatten)]"),
  "and names it before the flattened channel map, never inside it",
);
check(
  /#\[serde\(default, skip_serializing_if = "Option::is_none"\)\]\s*pub sound: Option<bool>,/.test(
    body,
  ),
  "an unanswered row parses and is written back with no key at all",
);
check(
  /#\[serde\(flatten\)\]\s*pub channels: BTreeMap<String, bool>,/.test(body),
  "the flattened map is still what makes the named field necessary",
);

// 2. The same shape on this side: an optional named field, so `undefined` reads
//    as unanswered rather than as a chosen off.
const unanswered: AgentLevelChoice = { level: "action_needed", bell: true, desktop: false };
check(unanswered.sound === undefined, "an unanswered row carries no answer");
check(
  levelSounds(unanswered, unanswered.level) === true,
  "and falls back to the chime that shipped for it",
);
const answered: AgentLevelChoice = { ...unanswered, sound: false };
check(levelSounds(answered, answered.level) === false, "a stored false is an answer, not an absence");

// 3. Nothing on the destination path can write the key. `sound` is not a
//    column at any roster size, so the writer that removes a channel key by
const roster = [
  { id: "telegram", label: "Telegram", control_available: true },
  { id: "discord", label: "Discord", control_available: true },
];
for (const columns of [destinationColumns([]), destinationColumns(roster)]) {
  check(
    !columns.some((column) => column.id === ("sound" as string)),
    "the chime is not one of the destination columns, at any roster size",
  );
}

// 4. The one writer for the chime moves the named field and touches nothing
//    else — no channel key appears, disappears, or changes.
const before: AgentLevelChoice[] = [
  { level: "verbose", bell: true, desktop: false, telegram: true },
  { level: "action_needed", bell: true, desktop: true },
];
const after = setLevelSound(before, "verbose", true);
check(after[0].sound === true, "the answer is written");
check(
  JSON.stringify({ ...after[0], sound: undefined }) ===
    JSON.stringify({ ...before[0], sound: undefined }),
  `nothing else on the row moved, got ${JSON.stringify(after[0])}`,
);
check(
  JSON.stringify(after[1]) === JSON.stringify(before[1]),
  "and no neighbouring row moved",
);
check(after[0].telegram === true, "the flattened channel key is untouched");

// 5. And the destination writer stays out of the chime, in both directions.
const channel = destinationColumns(roster).find((column) => column.id === "telegram");
check(channel, "the roster resolved a channel column");
if (channel) {
  const opened = setLevelDestination(after, "verbose", channel, true);
  check(opened[0].sound === true, "opening a channel leaves the chime answer standing");
  const closed = setLevelDestination(opened, "verbose", channel, false);
  check(!("telegram" in closed[0]), "closing a channel removes its key, as it always did");
  check(closed[0].sound === true, "and still leaves the chime answer standing");
}

console.log(`level_sound/stored_shape_is_a_named_field: ${checked} assertions passed`);
