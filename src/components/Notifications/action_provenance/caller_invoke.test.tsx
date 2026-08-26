// Run: npx tsx src/components/Notifications/action_provenance/caller_invoke.test.tsx
//
// THE DEFECT TEST. A caller-supplied action must not be able to reach the Tauri
// bridge from either surface that renders stored notification rows.
//
// It imports nothing the separation added, so it also runs against a tree that
// predates the split - where it fails, because both surfaces called
// `invoke(action.value, action.args)` on whatever the stored row named.

import {
  CALLER_SUPPLIED_INVOKE,
  SURFACES,
  assert,
  clickThrough,
  report,
  row,
} from "./fixture";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

async function run() {
  for (const surface of SURFACES) {
    const transcript = await clickThrough(surface, row([CALLER_SUPPLIED_INVOKE]), ["Go"]);

    // The button is present and was clicked: this passes because nothing can
    // reach a command, not because there was nothing to click.
    check(
      transcript.rendered.length === 1,
      `${surface}: the caller-supplied action should still render its button`,
    );
    check(
      transcript.invoked.length === 0,
      `${surface}: a caller-supplied action reached invoke with ` +
        `${JSON.stringify(transcript.invoked)}`,
    );
    check(
      transcript.opened.length === 0,
      `${surface}: a caller-supplied invoke must not be re-read as a URL either`,
    );
    check(
      transcript.answered.length === 0,
      `${surface}: a caller-supplied invoke is not an offered choice`,
    );
  }

  // The same claim over the whole caller-facing kind set rather than one
  // payload. Imported dynamically so the surface assertions above are what
  // decide this file on a tree that has no such module yet.
  const { CALLER_ACTION_KINDS } = await import("../../../lib/notificationActions");
  check(
    !CALLER_ACTION_KINDS.includes("invoke" as never),
    "the caller-facing kind set must not contain invoke",
  );

  report("action_provenance caller invoke", checked);
}

void run();
