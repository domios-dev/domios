// Run: npm run test:unit -- chatAskChoiceModel
import assert from "node:assert/strict";
import type { Notification } from "../../lib/types";
import { tabAsksEqual } from "../../store/multiplexer/selectors";
import { acceptsTap, chatAskChoiceState, isChoiceEnabled } from "./chatAskChoiceModel";

function ask(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "ask-1", tab_id: "asker", workspace_id: null, title: "Allow?", body: "Full question",
    severity: "Warning", level: "action_needed", timestamp: 1, read: false,
    actions: [{ type: "answer", label: "Yes" }, { type: "answer", label: "No" }],
    answer: { asked_from_tab: "asker", answered: null, delivered: false },
    ...overrides,
  };
}

const open = chatAskChoiceState(ask(), null);
assert.equal(open.phase, "open");
assert.ok(acceptsTap(open, "Yes", null) && acceptsTap(open, "No", null));

const tapped = chatAskChoiceState(ask(), "Yes");
assert.equal(tapped.phase, "sending");
assert.equal(tapped.chosen, "Yes");
assert.ok(!isChoiceEnabled(tapped, "Yes", null) && !isChoiceEnabled(tapped, "No", null));
assert.ok(!acceptsTap(tapped, "Yes", null), "a repeated tap while sending is a no-op");

const pending = chatAskChoiceState(ask({ answer_pending: true }), "No");
assert.equal(pending.phase, "sending");
assert.equal(pending.chosen, "No");

const answered = chatAskChoiceState(
  ask({ answer: { asked_from_tab: "asker", answered: "Yes", delivered: true } }), "No",
);
assert.equal(answered.phase, "answered");
assert.equal(answered.chosen, "Yes", "the recorded answer wins over a local tap");
assert.ok(!answered.showChoices && !acceptsTap(answered, "Yes", "Yes"));

const refused = chatAskChoiceState(ask({ answer_error: "Refused" }), "Yes");
assert.equal(refused.phase, "failed");
assert.equal(refused.error, "Refused");
assert.ok(acceptsTap(refused, "Yes", null) && acceptsTap(refused, "No", null), "an unrecorded refusal reopens every choice");

const undelivered = chatAskChoiceState(
  ask({ answer_error: "No agent", answer: { asked_from_tab: "asker", answered: "Yes", delivered: false } }), null,
);
assert.equal(undelivered.phase, "failed");
assert.ok(acceptsTap(undelivered, "Yes", "Yes") && !acceptsTap(undelivered, "No", "Yes"), "a recorded answer retries only itself");

assert.equal(chatAskChoiceState(ask({ resolved: true }), "Yes").phase, "resolved");
assert.equal(chatAskChoiceState(ask({ answer: { asked_from_tab: null, answered: null, delivered: false } }), null).phase, "undeliverable");

// The Chat rail selector must let every field the card reads through, or a tap never re-renders.
const base = ask();
assert.ok(tabAsksEqual([base], [{ ...base }]));
for (const changed of [
  { answer_pending: true },
  { answer_error: "Refused" },
  { resolved: true },
  { answer: { asked_from_tab: "asker", answered: null, delivered: true } },
] satisfies Partial<Notification>[]) {
  assert.ok(!tabAsksEqual([base], [{ ...base, ...changed }]), `selector must see ${Object.keys(changed)[0]}`);
}

console.log("chat ask choice model: passed");
