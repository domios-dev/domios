// Run: npm run test:unit -- notification_sound
//
// What an UNANSWERED level chimes. "Finished" is not a level on the shipped
// verbose/action_needed/failure axis, so the fallback keys on the two levels
// that already mean the agent is no longer working: `action_needed` (it is
// waiting on the user) and `failure` (it broke and stopped). Progress does not,
// because an agent still working has not finished, and an arrival naming no
// level claims nothing and so claims no completion either.
//
// This is now a DEFAULT and no longer a rule: a row that answers for itself
// never reaches it, which is level_sound/the_choice_beats_the_shipped_rule's
// claim. This file owns the fallback — flip `action_needed` in SHIPPED_SOUND and
// the first assertion below is what breaks.
import { assert, mountSound, report, unansweredLevels } from "./harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const OPTED_IN = true;

async function main() {
  const stopped = await mountSound(OPTED_IN, unansweredLevels());
  await stopped.arrive("action_needed");
  const afterActionNeeded = stopped.recorder.notes.length;
  check(afterActionNeeded > 0, "an agent waiting on the user is an agent that stopped, and sounds");

  await stopped.arrive("failure");
  check(
    stopped.recorder.notes.length === afterActionNeeded * 2,
    "an agent that broke and stopped sounds too, and each arrival sounds once",
  );
  await stopped.done();

  const working = await mountSound(OPTED_IN, unansweredLevels());
  await working.arrive("verbose");
  check(
    working.recorder.notes.length === 0,
    "progress does not stop the agent, so it must not announce a finish",
  );

  await working.arrive(null);
  check(
    working.recorder.notes.length === 0,
    "an arrival naming no level claims nothing about the agent and stays silent",
  );
  await working.done();

  report("notification_sound/sounds_when_the_agent_stopped", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
