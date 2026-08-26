// Run: npm run test:unit -- notification_sound
//
// THE DEFECT TEST. The enabling click used to be the ONLY thing that
// established the chime's audio context, so a sound already on in stored config
// — every restart, and anyone who set it once — had no gesture to be
// established by. The context then opened inside a daemon-event callback, which
// is not a gesture, so it started suspended and stayed silent.
//
// Nothing here touches the settings surface: the opt-in arrives as stored
// config, exactly as it does at startup. Pre-change the first case failed with
// zero notes, because the arrival was the first thing to open a context.

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
  //    same arrival is silent, because a context opened inside a daemon event
  //    starts suspended. This is the browser's rule, and it is why case 1 is a
  //    real claim rather than a stub agreeing with a stub.
  const ungestured = await mountStoredSound(ENABLED);
  await ungestured.arrive(STOPPED);
  check(
    ungestured.recorder.notes.length === 0,
    "without a gesture the context is suspended and nothing is heard",
  );
  check(
    ungestured.recorder.created > 0,
    "and it is silence from suspension, not from never reaching the player",
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
