import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CHAT_SCROLL_DIAGNOSTIC_BURST,
  CHAT_SCROLL_DIAGNOSTIC_REFILL_MS,
  createChatScrollDiagnosticDispatcher,
  type ChatScrollDiagnosticRecord,
} from "./chatScrollDiagnostic";

test("coalesces repeated same-frame diagnostics per scope and edge", () => {
  let now = 100;
  const queued: Array<() => void> = [];
  const emitted: ChatScrollDiagnosticRecord[] = [];
  const dispatch = createChatScrollDiagnosticDispatcher({
    now: () => now,
    scheduleFrame: (callback) => queued.push(callback),
    emit: (record) => emitted.push(record),
  });

  for (let index = 0; index < 25; index++) {
    now += 1;
    dispatch("tab:1", "scroll", { scrollTop: index * 10 });
  }
  dispatch("tab:1", "mode", { mode: "detached" });

  assert.equal(queued.length, 1);
  assert.equal(emitted.length, 0);

  queued.shift()!();
  assert.equal(emitted.length, 2);
  assert.deepEqual(emitted[0].state, { scrollTop: 240, coalesced: 24 });
  assert.deepEqual(emitted[1].state, { mode: "detached" });
});

test("bounds sustained scroll runs to the token budget and reports dropped counts on recovery", () => {
  let now = 0;
  const queued: Array<() => void> = [];
  const emitted: ChatScrollDiagnosticRecord[] = [];
  const dispatch = createChatScrollDiagnosticDispatcher({
    now: () => now,
    scheduleFrame: (callback) => queued.push(callback),
    emit: (record) => emitted.push(record),
  });

  // Simulate an 18-second scroll run generating 1,800 scroll events across 900 frames.
  for (let frame = 0; frame < 900; frame++) {
    now = frame * 20;
    dispatch("tab:1", "scroll", { scrollTop: frame * 2 });
    dispatch("tab:1", "scroll", { scrollTop: frame * 2 + 1 });
    while (queued.length > 0) queued.shift()!();
  }

  const maxAllowed = CHAT_SCROLL_DIAGNOSTIC_BURST + Math.ceil((900 * 20) / CHAT_SCROLL_DIAGNOSTIC_REFILL_MS);
  assert.ok(emitted.length <= maxAllowed, `expected <= ${maxAllowed} emissions, got ${emitted.length}`);
  assert.ok(emitted.length >= CHAT_SCROLL_DIAGNOSTIC_BURST);

  const last = emitted[emitted.length - 1].state as { dropped?: number; coalesced?: number };
  assert.ok((last.dropped ?? 0) > 0, "dropped events are surfaced when budget refills");
  assert.equal(last.coalesced, 1);
});
