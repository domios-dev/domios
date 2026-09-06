// The bell panel and the toast read the same stored rows, so a fix in one of
// them is a fix in neither. Both are driven from the one fixture here and their

import {
  CALLER_SUPPLIED_INVOKE,
  DAEMON_INVOKE,
  NON_INVOKE_KINDS,
  assert,
  clickThrough,
  report,
  row,
  type Transcript,
} from "./fixture";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const CASES: Array<{ name: string; actions: unknown[]; labels: string[] }> = [
  { name: "caller-supplied invoke", actions: [CALLER_SUPPLIED_INVOKE], labels: ["Go"] },
  { name: "daemon invoke", actions: [DAEMON_INVOKE], labels: ["Update"] },
  { name: "non-invoke kinds", actions: NON_INVOKE_KINDS, labels: ["Docs", "Later"] },
  {
    name: "mixed row",
    actions: [CALLER_SUPPLIED_INVOKE, DAEMON_INVOKE, ...NON_INVOKE_KINDS],
    labels: ["Go", "Update", "Docs", "Later"],
  },
];

function outward(transcript: Transcript) {
  return JSON.stringify({
    invoked: transcript.invoked,
    opened: transcript.opened,
    answered: transcript.answered,
  });
}

async function run() {
  for (const testCase of CASES) {
    const panel = await clickThrough("panel", row(testCase.actions), testCase.labels);
    const toast = await clickThrough("toast", row(testCase.actions), testCase.labels);

    check(
      panel.rendered.length === testCase.labels.length,
      `${testCase.name}: the panel should render every button under test`,
    );
    check(
      toast.rendered.length === testCase.labels.length,
      `${testCase.name}: the toast should render every button under test`,
    );
    check(
      outward(panel) === outward(toast),
      `${testCase.name}: the surfaces disagree - panel ${outward(panel)} vs toast ${outward(toast)}`,
    );
    check(
      panel.invoked.every((call) => call.command === "respond_provider_update_prompt"),
      `${testCase.name}: only a daemon command may reach the bridge from either surface`,
    );
  }

  report("action_provenance both surfaces", checked);
}

void run();
