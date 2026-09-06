/**
 * Run: npm run test:unit -- notification_sound
 *
 * The fate of the bare `notifications.sound` bool: RETAINED, as the MASTER MUTE
 * over per-level answers that now live on the rows.
 *
 * Retiring it was the alternative, and it loses three things. It is a mute:
 * one gesture silences everything and restores the answers intact, which three
 * per-level switches cannot do without destroying what they said. It is the
 * single moment the browser's audio permission has a gesture to hang off, and
 * splitting that across per-level switches is three places to get it wrong. And
 * a stored `sound = true` would read as silence, since an unanswered row is only
 * reachable while there is a master to be on — which is the exact regression the
 * per-level work exists to end.
 *
 * This file pins the master's shape, its default, and its one control. Which
 * levels chime is level_sound's claim, and is deliberately not restated here.
 */
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { defaultConfig } from "../../hooks/useConfig";
import { LOCAL_DESTINATIONS, destinationColumns } from "../notifyDestinations";
import { shouldSoundArrival } from "../notificationSound";
import { unansweredLevels } from "./harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  if (!condition) throw new Error(message);
}

const LEVELS = ["verbose", "action_needed", "failure", null];
const here = dirname(fileURLToPath(import.meta.url));
const read = (relative: string) => readFileSync(resolve(here, relative), "utf8");

// 1. Retained, not retired: the field still exists on both sides of the wire,
//    as a plain bool of the notifications section rather than a row of the grid.
check("sound" in defaultConfig().notifications, "the frontend config still carries the field");
const schema = read("../../../src-tauri/crates/codeterm-types/src/config/schema.rs");
const section = schema.slice(schema.indexOf("pub struct NotificationConfig"));
check(
  /pub sound: bool,/.test(section.slice(0, section.indexOf("\n}"))),
  "and so does the daemon schema, still as one bool for the whole section",
);
check(defaultConfig().notifications.sound === false, "and it is still off by default");

// 2. Not migrated into the DESTINATIONS: the chime is a named field of a row,
//    but it is not a place a message goes, so no roster size grows a column for
const registered = [
  { id: "telegram", label: "Telegram", control_available: true },
  { id: "discord", label: "Discord", control_available: false },
];
for (const columns of [destinationColumns([]), destinationColumns(registered)]) {
  check(
    !columns.some((destination) => destination.id === ("sound" as string)),
    "sound is not one of the destinations, at any roster size",
  );
}
check(
  LOCAL_DESTINATIONS.every((surface) => surface.id !== ("sound" as string)),
  "and it is not one of the surfaces the app draws either",
);

// 3. The mute dimension, exercised. Off is silence at every level however that
//    level answered, and the field is load-bearing rather than dead: on, at
const everyLevelChiming = unansweredLevels().map((row) => ({ ...row, sound: true }));
check(
  LEVELS.every((level) => !shouldSoundArrival(false, everyLevelChiming, level)),
  "muted, nothing sounds at any level even where every level chose to chime",
);
check(
  LEVELS.some((level) => shouldSoundArrival(true, everyLevelChiming, level)),
  "unmuted, the field decides something — it is not the dead switch it used to be",
);

// 4. One control, wherever Settings currently keeps it, and it is the shared
//    toggle. Found by search rather than by filename: the notification settings
const settingsDir = resolve(here, "../../components/Settings");
const holders = readdirSync(settingsDir)
  .filter((name) => /\.tsx$/.test(name) && !/\.test\.tsx$/.test(name))
  .map((name) => join(settingsDir, name))
  .filter((file) => readFileSync(file, "utf8").includes("config.notifications.sound"));
check(holders.length === 1, `exactly one Settings file owns the control, found ${holders.length}`);
const settings = readFileSync(holders[0], "utf8");
check(
  /<SwitchTile\s[\s\S]{0,240}?checked=\{config\.notifications\.sound\}/.test(settings),
  "the control is the shared switch tile bound to the field, not a new one",
);
check(
  /onUpdate\("notifications", \{ sound: v \}\)/.test(settings),
  "and it writes the bare field, never a grid row",
);
check(
  /import \{[^}]*\bSwitchTile\b[^}]*\} from "\.\/shared"/.test(settings),
  "the control comes from the shared settings rows",
);
const shared = read("../../components/Settings/shared.tsx");
check(shared.includes("LiquidToggle"), "the shared toggle is still the liquid toggle");

// 5. One reader, and it is handed the rows alongside the mute — the mute alone
//    is no longer a complete answer to what chimes.
const app = read("../../App.tsx");
check(
  /useNotificationSound\(\s*Boolean\(config\.notifications\?\.sound\),/.test(app),
  "the sole reader hands the mute to the arrival subscription",
);
check(
  /useNotificationSound\([\s\S]{0,120}config\.notifications\?\.agent_levels/.test(app),
  "and hands it the level rows in the same call",
);

console.log(`notification_sound/master_mute_retained_beside_the_rows: ${checked} assertions passed`);
