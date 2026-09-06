// Negative control. The kinds that never carried capability behave exactly as
// they did: a url opens, a dismiss does nothing, and a choice still delivers

import { adoptStoredActions } from "../../../lib/notificationActions";
import { assert, clickThrough, report, row } from "./fixture";

let checked = 0;
function check(condition: unknown, message: string): void {
  checked++;
  assert(condition, message);
}

const URL_ACTION = { type: "url", label: "Docs", value: "https://example.invalid" };
const DISMISS_ACTION = { type: "dismiss", label: "Later" };
const ANSWER_ACTION = { type: "answer", label: "Rebase" };

async function run() {
  const opened = await clickThrough("panel", row([URL_ACTION]), ["Docs"]);
  check(opened.opened.length === 1, "a url action still opens exactly one window");
  check(opened.opened[0] === URL_ACTION.value, "a url action still opens its own target");
  check(opened.invoked.length === 0, "a url action never reaches the bridge");

  const dismissed = await clickThrough("panel", row([DISMISS_ACTION]), ["Later"]);
  check(dismissed.invoked.length === 0, "a dismiss action never reaches the bridge");
  check(dismissed.opened.length === 0, "a dismiss action opens nothing");

  const answered = await clickThrough("panel", row([ANSWER_ACTION]), ["Rebase"]);
  check(answered.answered.length === 1, "a choice still delivers its label");
  check(answered.answered[0] === ANSWER_ACTION.label, "a choice delivers the label it offered");
  check(answered.invoked.length === 0, "a choice never reaches the generic action runner");

  // A vanishing toast must still not offer a choice - unchanged behaviour.
  const toast = await clickThrough("toast", row([ANSWER_ACTION]), ["Rebase"]);
  check(toast.rendered.length === 0, "the toast still offers no choice buttons");
  check(toast.answered.length === 0, "the toast still records no answer");

  // Ingress leaves the powerless kinds exactly as they arrived.
  for (const action of [URL_ACTION, DISMISS_ACTION, ANSWER_ACTION]) {
    check(
      JSON.stringify(adoptStoredActions([action])) === JSON.stringify([action]),
      `${action.type} must survive ingress byte for byte`,
    );
  }

  report("action_provenance non-invoke kinds", checked);
}

void run();
