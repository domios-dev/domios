// Sound reads the level row alone so routing and notification tone cannot diverge.
import { LOCAL_DESTINATIONS, destinationColumns } from "../notifyDestinations";
import { assert, mountSound, report } from "./harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const OPTED_IN = true;
async function main() {
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
