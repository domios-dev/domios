// The bell panel and the toast read the same stored rows, so a fix in one of them is a fix in neither.

import {
  CALLER_SUPPLIED_INVOKE,
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
  { name: "caller-supplied legacy action", actions: [CALLER_SUPPLIED_INVOKE], labels: [] },
  { name: "non-invoke kinds", actions: NON_INVOKE_KINDS, labels: ["Docs", "Later"] },
  {
    name: "mixed row",
    actions: [CALLER_SUPPLIED_INVOKE, ...NON_INVOKE_KINDS],
    labels: ["Docs", "Later"],
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
    check(panel.invoked.length === 0, `${testCase.name}: legacy actions never reach the bridge`);
  }

  report("action_provenance both surfaces", checked);
}

void run();
