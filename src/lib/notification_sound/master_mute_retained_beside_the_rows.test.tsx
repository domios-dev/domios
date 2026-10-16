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

// 1. Retained, not retired: the field still exists on both sides of the wire,
//    as a plain bool of the notifications section rather than a row of the grid.
check("sound" in defaultConfig().notifications, "the frontend config still carries the field");
check(defaultConfig().notifications.sound === false, "and it is still off by default");

// Keep the chime beside notification rows because it is not a destination and must not add a roster column.
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

// Off is silence at every level however that level answered, and the field is load-bearing rather than dead: on, at least one answer changes.
const everyLevelChiming = unansweredLevels().map((row) => ({ ...row, sound: true }));
check(
  LEVELS.every((level) => !shouldSoundArrival(false, everyLevelChiming, level)),
  "muted, nothing sounds at any level even where every level chose to chime",
);
check(
  LEVELS.some((level) => shouldSoundArrival(true, everyLevelChiming, level)),
  "unmuted, the field decides something — it is not the dead switch it used to be",
);

console.log(`notification_sound/master_mute_retained_beside_the_rows: ${checked} assertions passed`);
