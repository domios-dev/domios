// Opt-in stays opt-in. The master default is off in every place that answers
// for it, and a config that predates this change — absent field, or a stored
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { defaultConfig } from "../../hooks/useConfig";
import { shouldSoundArrival } from "../notificationSound";
import { assert, mountSound, report, unansweredLevels } from "./harness";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const here = dirname(fileURLToPath(import.meta.url));
const read = (relative: string) => readFileSync(resolve(here, relative), "utf8");

async function main() {
  // 1. The frontend default, the daemon default, and the written template all say off.
  check(defaultConfig().notifications.sound === false, "the frontend default is off");

  const schema = read("../../../src-tauri/crates/codeterm-types/src/config/schema.rs");
  check(/\bsound: false,/.test(schema), "the daemon default is off");
  check(
    /#\[serde\(default\)\]\s*pub struct NotificationConfig/.test(schema),
    "an absent field falls back to that default rather than failing to parse",
  );

  const template = read("../../../src-tauri/src/config/mod.rs");
  check(/^sound = false\b/m.test(template), "the written template ships it off");

  // 2. Absent or legacy: neither a missing master nor a stored `false` sounds,
  //    whatever the level says and however its own row answers.
  const rows = unansweredLevels();
  const allOn = rows.map((row) => ({ ...row, sound: true }));
  for (const level of ["verbose", "action_needed", "failure", null]) {
    check(
      !shouldSoundArrival(undefined, rows, level),
      `an absent answer stays silent at ${String(level)}`,
    );
    check(
      !shouldSoundArrival(false, rows, level),
      `a stored off answer stays silent at ${String(level)}`,
    );
    check(
      !shouldSoundArrival(false, allOn, level),
      `and stays silent at ${String(level)} even where the level chose to chime`,
    );
  }

  // 3. And behaviourally, through the shipped hook: opted out, the arrival that
  //    would otherwise sound loudest is silent.
  const optedOut = await mountSound(false, allOn);
  await optedOut.arrive("failure");
  check(optedOut.recorder.notes.length === 0, "opted out, nothing sounds");
  await optedOut.arrive("action_needed");
  check(optedOut.recorder.notes.length === 0, "still nothing");
  await optedOut.done();

  report("notification_sound/opt_in_default_and_legacy_config", checked);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
