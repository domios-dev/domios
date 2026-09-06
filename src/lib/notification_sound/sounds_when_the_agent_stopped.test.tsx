// What an UNANSWERED level chimes. "Finished" is not a level on the shipped
// verbose/action_needed/failure axis, so the fallback keys on the two levels
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
