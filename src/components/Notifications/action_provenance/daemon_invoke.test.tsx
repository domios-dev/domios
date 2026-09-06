// The live daemon-constructed invoke - the provider-update prompt - still runs
// end to end from the surface the user actually clicks.

import { DAEMON_COMMANDS } from "../../../lib/notificationActions";
import { DAEMON_INVOKE, assert, clickThrough, report, row } from "./fixture";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

async function run() {
  const transcript = await clickThrough("panel", row([DAEMON_INVOKE]), ["Update"]);

  check(transcript.rendered.length === 1, "the daemon action should render its button");
  check(
    transcript.invoked.length === 1,
    `a daemon-constructed invoke should reach the bridge exactly once, got ${transcript.invoked.length}`,
  );
  check(
    transcript.invoked[0]?.command === "respond_provider_update_prompt",
    "the daemon action should reach its own command",
  );
  const args = transcript.invoked[0]?.args as { promptId?: unknown; accept?: unknown };
  check(args?.promptId === "prompt-1", "the prompt id should survive the runner");
  check(args?.accept === true, "the decision should survive the runner");

  // The allowlist is a stated, closed set - not an open string field.
  check(DAEMON_COMMANDS.length === 1, "the daemon command set holds exactly one command today");
  check(
    DAEMON_COMMANDS.includes("respond_provider_update_prompt"),
    "the provider-update prompt is the daemon command in the set",
  );

  report("action_provenance daemon invoke", checked);
}

void run();
