// Run: npm run test:unit -- notification_sound
//
// The sound cannot disagree with routing, and the reason changed: it used to be
// that the arrival event was minted inside the bell gate's admitted arm, so a
// level routed away from the bell had no path to a sound. That gate is gone —
// it was gating the panel's only live path, not a destination — and the arrival
// is now unconditional. What replaced the old guarantee is a stronger one:
// there is no routing input to the sound at all. The LEVEL decides, and nothing
// else can.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { LOCAL_DESTINATIONS, destinationColumns } from "../notifyDestinations";
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
    if (/\.test\.tsx?$/.test(entry.name) || /harness\.tsx$/.test(entry.name)) continue;
    found.push(full);
  }
  return found;
}

async function main() {
  // 1. The daemon side: one emit, and nothing stands in front of it. A gate here
  //    would be a gate on the panel's only live path, which is the defect that
  //    retired it — so this pins that no successor gate grew back.
  const gate = read("../../../src-tauri/src/service/notification.rs");
  check(
    (gate.match(/\bemit\(payload\)/g) ?? []).length === 1,
    "the arrival event is emitted from exactly one place",
  );
  check(
    /^ {4}emit\(payload\);$/m.test(gate),
    "at the function body's own indentation, so it sits in no branch",
  );
  check(
    !/BellPolicy|bell::admit|BellAdmission/.test(gate),
    "and the policy that used to stand in front of it is gone, not merely bypassed",
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
    "and that subscription is the daemon's arrival",
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

  // 4. The decision module reads no routing config of its own, so the level is
  //    genuinely the whole input rather than one of two answers that could drift.
  const decision = read("../notificationSound.ts");
  check(!decision.includes("agent_levels"), "the sound never reads the levels grid");
  check(!decision.includes("notifyDestinations"), "and never reads the destination set");

  // 5. Nor is it a column anyone could set: `sound` is not a destination, at any
  //    roster size, and the bell is no longer one either.
  check(
    destinationColumns([{ id: "telegram", label: "Telegram", control_available: true }]).every(
      (destination) => destination.id !== ("sound" as string),
    ),
    "sound is not a destination, so it has no column",
  );
  check(
    LOCAL_DESTINATIONS.every((surface) => surface.id !== ("bell" as string)),
    "and the bell is not one either: the panel takes every record, so nothing is offered",
  );

  // 6. And behaviourally: opted in, mounted, with no arrival delivered, nothing
  //    sounds. The event is the whole trigger.
  const idle = await mountSound(OPTED_IN);
  check(idle.recorder.notes.length === 0, "no arrival, no sound");
  await idle.done();

  report("notification_sound/sound_is_decided_by_the_level_alone", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
