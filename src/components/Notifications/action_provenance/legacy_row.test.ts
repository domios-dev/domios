// Stripping rather than rejecting the whole row is deliberate: the title and body are the user's message and a real one, while the button is the only part that ever carried capability.

import { adoptStoredActions } from "../../../lib/notificationActions";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  if (!condition) throw new Error(message);
}

const LEGACY_INVOKE = {
  type: "invoke",
  label: "Go",
  value: "clear_notifications",
  args: { id: "notice-1" },
};

function run() {
  const legacyRow = [
    LEGACY_INVOKE,
    { type: "url", label: "Docs", value: "https://example.invalid" },
    { type: "dismiss", label: "Later" },
    { type: "answer", label: "Rebase" },
  ];
  const adopted = adoptStoredActions(legacyRow) ?? [];

  check(adopted.length === 3, `the legacy invoke should be dropped, got ${JSON.stringify(adopted)}`);
  check(
    adopted.every((action) => (action.type as string) !== "invoke"),
    "a legacy invoke must not survive ingress",
  );
  check(
    JSON.stringify(adopted) === JSON.stringify(legacyRow.slice(1)),
    "every other kind on a legacy row survives byte for byte",
  );
  check(
    !JSON.stringify(adopted).includes("clear_notifications"),
    "no field of an adopted row may still carry the legacy command name",
  );

  // A row whose ONLY action was a legacy invoke keeps its message and loses its
  // button - the row itself is not rejected.
  check(
    JSON.stringify(adoptStoredActions([LEGACY_INVOKE])) === "[]",
    "a row carrying only a legacy invoke adopts to an empty action set",
  );

  // A command outside the daemon set is the same case, whatever field names it.
  for (const outside of ["clear_notifications", "", "RetiredAgentAction"]) {
    check(
      JSON.stringify(adoptStoredActions([{ type: "invoke", label: "Go", command: outside }])) === "[]",
      `an invoke naming ${outside} is outside the daemon set and must be dropped`,
    );
  }

  check(adoptStoredActions(undefined) === undefined, "a row with no actions adopts to none");
  check(adoptStoredActions("nope") === undefined, "a non-array action set adopts to none");

  console.log(`action_provenance legacy row: ${checked} assertions passed`);
}

run();
