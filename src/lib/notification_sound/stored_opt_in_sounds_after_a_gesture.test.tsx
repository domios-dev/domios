// THE DEFECT TEST. The enabling click used to be the ONLY thing that
// established the chime's audio context, so a sound already on in stored config

import {
  assert,
  mountStoredSound,
  report,
} from "./gesture_harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const ENABLED = true;
const STOPPED = "action_needed";

async function main() {
  // 1. Stored opt-in, no settings interaction, one gesture, one arrival: audible.
  const stored = await mountStoredSound(ENABLED);
  check(stored.recorder.notes.length === 0, "mounting alone sounds nothing");
  await stored.gesture();
  await stored.arrive(STOPPED);
  check(
    stored.recorder.notes.length > 0,
    "a gesture before the arrival is what makes a stored opt-in audible",
  );
  await stored.done();

  // 2. The fixture is faithful, so the defect is observable: with NO gesture the
  //    same arrival is silent, because only a gesture can start a context that
  const ungestured = await mountStoredSound(ENABLED);
  await ungestured.arrive(STOPPED);
  check(
    ungestured.recorder.notes.length === 0,
    "without a gesture nothing is heard",
  );
  await ungestured.gesture();
  await ungestured.arrive(STOPPED);
  check(
    ungestured.recorder.notes.length > 0,
    "and the same arrival after a gesture is heard, so it was reaching the player all along",
  );
  await ungestured.done();

  // 3. Off means no audio resource at all: the gesture path opens nothing.
  const off = await mountStoredSound(false);
  check(off.listening() === 0, "nothing listens for a gesture while the sound is off");
  await off.gesture();
  check(
    off.recorder.created === 0,
    `a disabled sound creates no audio context, got ${off.recorder.created}`,
  );
  await off.arrive(STOPPED);
  check(off.recorder.notes.length === 0, "and an arrival stays silent");
  await off.done();

  // 4. Established at most once, and it stops listening once it has.
  const once = await mountStoredSound(ENABLED);
  check(
    once.listening() === 2,
    `both gesture events are listened for while unestablished, got ${once.listening()}`,
  );
  await once.gesture();
  check(
    once.listening() === 0,
    `the listener is removed once it has fired, got ${once.listening()} still registered`,
  );
  check(once.recorder.created === 1, `one context, got ${once.recorder.created}`);
  await once.gesture("keydown");
  check(
    once.recorder.created === 1,
    `a later gesture opens nothing more, got ${once.recorder.created}`,
  );
  await once.arrive(STOPPED);
  check(once.recorder.notes.length > 0, "and the one established context is what plays");
  await once.done();

  // 5. Either gesture establishes it — the keyboard is a gesture too.
  const keyed = await mountStoredSound(ENABLED);
  await keyed.gesture("keydown");
  check(keyed.listening() === 0, "a key stops the listening just as a pointer does");
  await keyed.arrive(STOPPED);
  check(keyed.recorder.notes.length > 0, "and makes the next arrival audible");
  await keyed.done();

  report("notification_sound/stored_opt_in_sounds_after_a_gesture", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
