/**
 * Run: npm run test:unit -- level_sound
 *
 * THE DEFECT TEST for the per-level chime.
 *
 * The chime used to key on a hardcoded table — `action_needed` and `failure`
 * sounded, `verbose` did not — that no configuration could contradict. Now each
 * level answers for itself, and the answer wins EVEN WHERE IT DISAGREES with
 * that table, which is the only thing that proves the choice is honoured rather
 * than the old rule. So every case below is a disagreement.
 *
 * The table survives as the answer an UNANSWERED row falls back to, and the
 * last case pins that it is a default and never an override.
 */
import {
  assert,
  levelsAnswering,
  mountSound,
  report,
  unansweredLevels,
} from "../notification_sound/harness";
import { shippedSound } from "../notificationSound";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const MASTER_ON = true;
const LEVELS = ["verbose", "action_needed", "failure"];

async function main() {
  // 1. The exact inverse of the old rule, level by level. Progress chimes; the
  //    two levels the rule chimed for stay silent.
  const inverted = await mountSound(
    MASTER_ON,
    levelsAnswering({ verbose: true, action_needed: false, failure: false }),
  );
  await inverted.arrive("verbose");
  const afterVerbose = inverted.recorder.notes.length;
  check(
    afterVerbose > 0,
    "progress chimes because the user chose it, against the rule that silenced it",
  );
  await inverted.arrive("action_needed");
  check(
    inverted.recorder.notes.length === afterVerbose,
    "action_needed stays silent because the user chose it, against the rule that sounded it",
  );
  await inverted.arrive("failure");
  check(
    inverted.recorder.notes.length === afterVerbose,
    "and so does failure — the old table no longer overrides anything",
  );
  await inverted.done();

  // 2. And back the other way, so case 1 is a choice being honoured rather than
  //    the mapping simply having been flipped in place.
  const agreeing = await mountSound(
    MASTER_ON,
    levelsAnswering({ verbose: false, action_needed: true, failure: true }),
  );
  await agreeing.arrive("verbose");
  check(agreeing.recorder.notes.length === 0, "verbose off is verbose silent");
  await agreeing.arrive("action_needed");
  const afterActionNeeded = agreeing.recorder.notes.length;
  check(afterActionNeeded > 0, "action_needed on is action_needed audible");
  await agreeing.arrive("failure");
  check(
    agreeing.recorder.notes.length === afterActionNeeded * 2,
    "failure on sounds too, and each arrival sounds exactly once",
  );
  await agreeing.done();

  // 3. One level answered leaves its neighbours alone: the answer is per row,
  //    not a switch the whole grid shares.
  const onlyVerbose = await mountSound(MASTER_ON, levelsAnswering({ verbose: true }));
  await onlyVerbose.arrive("verbose");
  const soleAnswer = onlyVerbose.recorder.notes.length;
  check(soleAnswer > 0, "the answered level chimes");
  await onlyVerbose.arrive("action_needed");
  check(
    onlyVerbose.recorder.notes.length > soleAnswer,
    "and an unanswered neighbour still falls back to the chime that shipped",
  );
  await onlyVerbose.done();

  // 4. The master mute is a mute, not a second answer: every level chosen ON is
  //    still silent while it is off, and the answers are untouched by it.
  const muted = await mountSound(
    false,
    levelsAnswering({ verbose: true, action_needed: true, failure: true }),
  );
  for (const level of LEVELS) await muted.arrive(level);
  check(muted.recorder.notes.length === 0, "muted, no level chimes however it answered");
  await muted.done();

  // 5. An arrival naming no level has no row and therefore no answer, so it
  //    borrows none.
  const unlevelled = await mountSound(MASTER_ON, levelsAnswering({ verbose: true }));
  await unlevelled.arrive(null);
  check(unlevelled.recorder.notes.length === 0, "an arrival naming no level stays silent");
  await unlevelled.done();

  // The fallback is a default, never an override: an unanswered set reproduces the shipped table exactly, which is what makes cases 1 and 2 disagreements rather than restatements.
  const shipped = await mountSound(MASTER_ON, unansweredLevels());
  let heard = 0;
  for (const level of LEVELS) {
    const before = shipped.recorder.notes.length;
    await shipped.arrive(level);
    const sounded = shipped.recorder.notes.length > before;
    check(
      sounded === shippedSound(level),
      `an unanswered ${level} falls back to the shipped answer, not to silence`,
    );
    if (sounded) heard++;
  }
  check(heard === 2, `the shipped table is still two of three levels, got ${heard}`);
  await shipped.done();

  report("level_sound/the_choice_beats_the_shipped_rule", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
