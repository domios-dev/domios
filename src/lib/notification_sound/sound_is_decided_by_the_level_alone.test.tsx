// Run: npm run test:unit -- notification_sound
//
// The sound cannot disagree with routing, and the reason has changed twice. It
// used to be that the arrival event was minted inside the bell gate's admitted
// arm, so a level routed away from the bell had no path to a sound. That gate is
// gone — it was gating the panel's only live path, not a destination — and the
// arrival is now unconditional.
//
// What holds today: the sound reads the LEVEL ROWS and nothing else. It is not a
// destination column, no roster can grow one, and the row it reads is the one
// naming the arriving level. So there is still exactly one input, and it is the
// level's own answer.
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

  // 2. Every daemon producer crosses the same event boundary.
  const producers = gate
    .split(/\r?\n/)
    .filter((line) => line.includes('"notification-created"'));
  check(producers.length === 1, "the gate names the arrival event once");
  check(
    /fn notify_created\(&self, payload: serde_json::Value\)/.test(gate) &&
      /self\.notify_event\("notification-created", payload\);/.test(gate),
    "the arrival event has one daemon helper",
  );
  check(
    (gate.match(/self\.notify_created\(/g) ?? []).length === 3,
    "creation and coalesced updates share the arrival helper",
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

  // 4. The decision module reads the level rows and NOTHING about where a level
  //    is routed, so the two cannot drift into disagreement. It is one input, and
  //    it is the row naming the arriving level.
  const decision = read("../notificationSound.ts");
  check(!decision.includes("notifyDestinations"), "the sound never reads the destination set");
  check(!decision.includes("isRouted"), "and never asks whether a level is routed anywhere");
  check(
    (decision.match(/levels\.find\(/g) ?? []).length === 1,
    "and it selects that row in exactly one place",
  );

  // 5. Nor is it a column anyone could set: `sound` is a NAMED field of the row,
  //    never a destination at any roster size, and the bell is no longer one either.
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
