import assert from "node:assert/strict";
import { composerDelayedModel } from "./composerDelayedModel";
import type { DelayedMessage } from "./delayedSend";

const base: DelayedMessage = { id: "future", tabId: "tab", text: "body", scheduledAtMs: 101, createdAtMs: 1, state: "queued", sessionChanged: false };
const list: DelayedMessage[] = [base, { ...base, id: "due", scheduledAtMs: 100 }, { ...base, id: "past", scheduledAtMs: 99 }, { ...base, id: "sending", state: "sending" }, { ...base, id: "stash", state: "stashed", scheduledAtMs: 0 }, { ...base, id: "failed", state: "failed", scheduledAtMs: 0 }, { ...base, id: "system:heartbeat:plan:tab" }, { ...base, id: "projection", origin: "system" }];
const model = composerDelayedModel(list, 100);
assert.deepEqual(model.chips.map(message => message.id), ["failed", "future", "stash"]);
assert.equal(model.count, 3);
assert.equal(model.alert, true);
assert.deepEqual(composerDelayedModel([...list].reverse(), 100), model);
assert.equal(composerDelayedModel([base], 100).alert, false);
assert.equal(composerDelayedModel([base], 101).count, 0);
assert.equal(list.length, 8);
console.log("composerDelayedModel: all assertions passed");
