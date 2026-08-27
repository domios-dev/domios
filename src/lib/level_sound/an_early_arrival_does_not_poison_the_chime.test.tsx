/**
 * Run: npm run test:unit -- level_sound
 *
 * THE DEFECT TEST for the silence.
 *
 * The arrival handler used to fall back to opening an audio context ITSELF when
 * the gesture had not established one yet, and then cached it. A browser starts
 * a context opened outside a user gesture SUSPENDED, and a suspended context is
 * silence — so the first arrival that beat the first click left a dead context
 * cached ahead of the live one a later gesture established. Every arrival after
 * that read the cache and was silent. Not a missed first chime: permanent
 * silence, on a build whose config, event, level and gesture were all correct.
 *
 * The fixture models the browser's rule (see `gesture_harness`), which is what
 * makes the first case observable at all. Pre-change it failed twice over: a
 * context was created without a gesture, and the post-gesture arrival stayed
 * silent because that dead context was still cached.
 */
import { assert, mountStoredSound, report } from "../notification_sound/gesture_harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const MASTER_ON = true;
const CHIMES = "action_needed";

async function main() {
  // 1. An arrival lands before anything has been clicked — an agent notification
  //    while the window sits unfocused, which is every background agent.
  const early = await mountStoredSound(MASTER_ON);
  await early.arrive(CHIMES);
  check(early.recorder.notes.length === 0, "with no gesture yet, that arrival is silent");
  check(
    early.recorder.created === 0,
    `and it opens no audio context of its own to go dead in, got ${early.recorder.created}`,
  );
  check(
    early.listening() > 0,
    "the gesture listener is still armed, so the chime has not given up",
  );

  // 2. The user then clicks, as this user has many times. The NEXT arrival must
  //    be heard. This is the assertion the shipped build failed.
  await early.gesture();
  check(early.recorder.created === 1, `the gesture opens exactly one context, got ${early.recorder.created}`);
  await early.arrive(CHIMES);
  check(
    early.recorder.notes.length > 0,
    "a gesture after an early arrival makes the chime audible — it was not poisoned",
  );

  // 3. And it keeps working: the established context is reused, not replaced,
  //    so the recovery is a state the app reaches rather than one arrival's luck.
  const afterFirst = early.recorder.notes.length;
  await early.arrive(CHIMES);
  check(
    early.recorder.notes.length === afterFirst * 2,
    "every later arrival sounds on that same context",
  );
  check(early.recorder.created === 1, `and no second context was ever opened, got ${early.recorder.created}`);
  await early.done();

  // 4. The claim case 1 replaced: silence before a gesture is the browser's
  //    rule and not the arrival being filtered out. Same level, same rows, same
  //    hook — the only difference is the gesture, and it is the whole difference.
  const gestured = await mountStoredSound(MASTER_ON);
  await gestured.gesture();
  await gestured.arrive(CHIMES);
  check(
    gestured.recorder.notes.length > 0,
    "the arrival that was silent before a gesture reaches the player after one",
  );
  await gestured.done();

  report("level_sound/an_early_arrival_does_not_poison_the_chime", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
