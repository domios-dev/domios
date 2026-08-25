// Run: npm run test:unit -- notification_sound
//
// The sound cannot disagree with routing, and this pins the mechanism rather
// than a coincidence of the current defaults: the daemon produces the arrival
// event ONLY inside the bell gate's admitted arm, and the arrival event is the
// only thing that reaches the player. A level routed away from the bell
// therefore has no path to a sound at all — there is nothing to keep in sync.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { NOTIFY_DESTINATIONS } from "../notifyDestinations";
import { assert, mountSound, report } from "./harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const OPTED_IN = true;
const here = dirname(fileURLToPath(import.meta.url));
const src = resolve(here, "../..");
const read = (relative: string) => readFileSync(resolve(here, relative), "utf8");

function productionFiles(directory: string, found: string[] = []): string[] {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const full = join(directory, entry.name);
    if (entry.isDirectory()) {
      productionFiles(full, found);
      continue;
    }
    if (!/\.tsx?$/.test(entry.name)) continue;
    if (/\.test\.tsx?$/.test(entry.name) || entry.name === "harness.tsx") continue;
    found.push(full);
  }
  return found;
}

async function main() {
  // 1. The daemon side: one emit, and it lives in the admitted arm. A level the
  //    user routed away from the bell reaches the refused arm, which emits nothing.
  const gate = read("../../../src-tauri/src/service/notification.rs");
  check(
    /bell::BellAdmission::Allowed\s*=>\s*emit\(payload\)/.test(gate),
    "the arrival event is emitted from the bell-admitted arm",
  );
  check(
    (gate.match(/\bemit\(/g) ?? []).length === 1,
    "and that is the only emit in the gate, so a refused level cannot reach one",
  );
  const refused = gate.slice(gate.indexOf("BellAdmission::LevelDisabled"));
  check(
    refused.length > 0 && !refused.includes("emit("),
    "the refused arm logs the level and stops",
  );

  // 2. Nothing else on the daemon side mints that event name.
  const producers = gate
    .split(/\r?\n/)
    .filter((line) => line.includes('"notification-created"'));
  check(producers.length > 0, "the gate names the arrival event");
  check(
    producers.every((line) =>
      /\|payload\|\s*self\.notify_event\("notification-created", payload\)/.test(line),
    ),
    "every producer of the arrival event is the gate's own emit closure",
  );

  // 3. The frontend side: the arrival event is the only trigger, and the player
  //    has exactly one caller.
  const hook = read("../../hooks/useNotificationSound.ts");
  check(
    (hook.match(/\blisten</g) ?? []).length === 1,
    "the hook subscribes once and to nothing else",
  );
  check(
    /const ARRIVAL_EVENT = "notification-created";/.test(hook),
    "and that subscription is the daemon's bell arrival",
  );
  const callers = productionFiles(src).filter((file) =>
    readFileSync(file, "utf8").includes("playArrivalChime"),
  );
  check(
    callers.length === 2,
    `only the module and its one caller reach the player, found ${callers.length}`,
  );
  check(
    callers.some((file) => file.endsWith(join("hooks", "useNotificationSound.ts"))),
    "and the caller is the arrival subscription",
  );

  // 4. The decision module reads no routing config of its own, so there is no
  //    second answer to "where may this level go" that could drift from the grid.
  const decision = read("../notificationSound.ts");
  check(!decision.includes("agent_levels"), "the sound never reads the levels grid");
  check(!decision.includes("notifyDestinations"), "and never reads the destination set");

  // 5. The config cannot even express "sound for a level that reaches nothing":
  //    there is no fourth column to set.
  check(
    NOTIFY_DESTINATIONS.every((destination) => destination.id !== ("sound" as string)),
    "sound is not a destination, so it has no column",
  );
  check(NOTIFY_DESTINATIONS.length === 3, "the destination set is unchanged");

  // 6. And behaviourally: opted in, mounted, with no arrival delivered, nothing
  //    sounds. The event is the whole trigger.
  const idle = await mountSound(OPTED_IN);
  check(idle.recorder.notes.length === 0, "no arrival, no sound");
  await idle.done();

  report("notification_sound/silenced_level_cannot_sound", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
