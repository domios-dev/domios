// Run: npm run test:unit -- notification_sound
//
// The fate of the bare `notifications.sound` bool: RETAINED, and given the
// meaning it never had. It stays outside `agent_levels` on purpose — it is
// presentation, next to `show_bell`, not a destination — so this pins that no
// fourth column appeared and that the field is the whole opt-in and is not
// inert again.
//
// Deliberately says nothing about WHICH levels sound: that is the defect test's
// claim, and duplicating it here would make a mutation of the level mapping
// break two files instead of the one that owns it.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { defaultConfig } from "../../hooks/useConfig";
import { NOTIFY_DESTINATIONS } from "../notifyDestinations";
import { shouldSoundArrival } from "../notificationSound";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  if (!condition) throw new Error(message);
}

const OPTED_IN = true;
const LEVELS = ["verbose", "action_needed", "failure", null];
const here = dirname(fileURLToPath(import.meta.url));
const read = (relative: string) => readFileSync(resolve(here, relative), "utf8");

// 1. Retained, not retired: the field still exists on both sides of the wire.
check("sound" in defaultConfig().notifications, "the frontend config still carries the field");
const schema = read("../../../src-tauri/crates/codeterm-types/src/config/schema.rs");
check(/pub sound: bool,/.test(schema), "and so does the daemon schema");

// 2. Not migrated into the matrix: no fourth column, on either side.
const row = { level: "failure", bell: true, desktop: true, telegram: true };
check(
  Object.keys(row).length === 1 + NOTIFY_DESTINATIONS.length,
  "a grid row is a level plus one cell per destination, and nothing more",
);
check(
  !NOTIFY_DESTINATIONS.some((destination) => destination.id === ("sound" as string)),
  "sound is not one of those destinations",
);
const destinations = read("../../../src-tauri/crates/codeterm-types/src/notification.rs");
check(
  /pub const ALL: \[Self; 3\] = \[Self::Bell, Self::Desktop, Self::Telegram\];/.test(destinations),
  "the daemon's destination set gained no member",
);
const agentLevelRow = schema.slice(schema.indexOf("pub struct AgentLevelChoice"));
check(
  !agentLevelRow.slice(0, agentLevelRow.indexOf("\n}")).includes("sound"),
  "and no sound cell was smuggled onto the row",
);

// 3. The switch dimension, exercised. Off is silence at every level, and the
//    field is load-bearing rather than dead again — turning it on changes at
//    least one answer. Which levels those are is the defect test's claim.
check(
  LEVELS.every((level) => !shouldSoundArrival(false, level)),
  "off, nothing sounds at any level",
);
check(
  LEVELS.some((level) => shouldSoundArrival(OPTED_IN, level)),
  "on, the field decides something — it is not the dead switch it used to be",
);

// 4. One control, wherever Settings currently keeps it, and it is the shared
//    toggle. Found by search rather than by filename: the notification settings
//    have already moved once, and a hardcoded path would have passed that move
//    while pointing at a file that no longer holds the control.
const settingsDir = resolve(here, "../../components/Settings");
const holders = readdirSync(settingsDir)
  .filter((name) => /\.tsx$/.test(name) && !/\.test\.tsx$/.test(name))
  .map((name) => join(settingsDir, name))
  .filter((file) => readFileSync(file, "utf8").includes("config.notifications.sound"));
check(holders.length === 1, `exactly one Settings file owns the control, found ${holders.length}`);
const settings = readFileSync(holders[0], "utf8");
check(
  /<Toggle\s+label="Sound[^"]*"\s+checked=\{config\.notifications\.sound\}/.test(settings),
  "the control is the shared toggle, not a new one",
);
check(
  /onUpdate\("notifications", \{ sound: v \}\)/.test(settings),
  "and it writes the bare field, never a grid row",
);
check(
  /import \{[^}]*\bToggle\b[^}]*\} from "\.\/shared"/.test(settings),
  "the toggle comes from the shared settings rows",
);
const shared = read("../../components/Settings/shared.tsx");
check(shared.includes("LiquidToggle"), "the shared toggle is still the liquid toggle");

// 5. One reader.
const app = read("../../App.tsx");
check(
  /useNotificationSound\(Boolean\(config\.notifications\?\.sound\)\)/.test(app),
  "the sole reader hands the field to the arrival subscription",
);

console.log(`notification_sound/bare_bool_retained_not_a_column: ${checked} assertions passed`);
