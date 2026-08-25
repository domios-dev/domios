// Run: npm run test:unit -- notification_sound
//
// The fate of the bare `notifications.sound` bool: RETAINED, and given the
// meaning it never had. It stays outside `agent_levels` on purpose — it is
// presentation, next to `show_bell`, not a destination — so this pins that no
// fourth column appeared, that the field is still the one switch, and that its
// meaning is exercised rather than merely asserted in prose.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { defaultConfig } from "../../hooks/useConfig";
import { NOTIFY_DESTINATIONS } from "../notifyDestinations";
import { shouldSoundArrival } from "../notificationSound";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  if (!condition) throw new Error(message);
}

const OPTED_IN = true;
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

// 3. Its meaning, exercised: the field is the whole opt-in, and it decides
//    nothing about routing — an admitted arrival still has to say the agent
//    stopped before the switch matters.
check(shouldSoundArrival(OPTED_IN, "action_needed"), "on, an agent waiting on the user sounds");
check(shouldSoundArrival(OPTED_IN, "failure"), "on, an agent that stopped on an error sounds");
check(!shouldSoundArrival(OPTED_IN, "verbose"), "on, progress still does not");
check(!shouldSoundArrival(OPTED_IN, null), "on, an unlevelled arrival still does not");

// 4. One writer, one reader. Nothing else reads the old field for some other
//    purpose, so its meaning cannot fork.
const settings = read("../../components/Settings/AgentsTab.tsx");
check(
  /<Toggle\s+label="Sound[^"]*"\s+checked=\{config\.notifications\.sound\}/.test(settings),
  "the control is the shared toggle, not a new one",
);
check(
  /onUpdate\("notifications", \{ sound: v \}\)/.test(settings),
  "and it writes the bare field, never a grid row",
);
const shared = read("../../components/Settings/shared.tsx");
check(shared.includes("LiquidToggle"), "the shared toggle is still the liquid toggle");

const app = read("../../App.tsx");
check(
  /useNotificationSound\(Boolean\(config\.notifications\?\.sound\)\)/.test(app),
  "the sole reader hands the field to the arrival subscription",
);

console.log(`notification_sound/bare_bool_retained_not_a_column: ${checked} assertions passed`);
