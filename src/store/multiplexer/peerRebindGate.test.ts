// Run: npx tsx src/store/multiplexer/peerRebindGate.test.ts
// The write gate means "we hold an accepted snapshot of the current origin".

import { createPeerRebindGate } from "./peerRebindGate";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  [ok] ${name}`);
  } catch (e) {
    failures++;
    console.error(`  [x] ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function build() {
  const announced: Array<{ switchId: number; success: boolean }> = [];
  let unlocks = 0;
  const gate = createPeerRebindGate({
    unlock: () => { unlocks++; },
    announce: (switchId, success) => announced.push({ switchId, success }),
  });
  return { gate, announced, unlocks: () => unlocks };
}

console.log("peer rebind gate");

test("a rebind that succeeds unlocks on its own accepted snapshot", () => {
  const h = build();
  h.gate.open(1);
  h.gate.acceptedSnapshot(1);
  assert(h.unlocks() === 1, "gate should unlock");
  assert(h.announced[0]?.success === true, "ready announced as success");
  assert(h.gate.openFor() === null, "gate closed");
});

test("a refused rebind stays locked and open", () => {
  const h = build();
  h.gate.open(2);
  h.gate.refused(2);
  assert(h.unlocks() === 0, "a refusal must not unlock");
  assert(h.announced.length === 0, "a refusal is not-yet, so nothing is announced");
  assert(h.gate.openFor() === 2, "the gate stays open for this switch");
});

test("after a refusal the next accepted snapshot unlocks", () => {
  const h = build();
  h.gate.open(3);
  h.gate.refused(3);
  // Whatever event produces it -- an ordinary mux-state-changed refresh.
  h.gate.acceptedSnapshot(3);
  assert(h.unlocks() === 1, "the next accepted snapshot unlocks");
  assert(h.announced[0]?.success === true, "and announces success");
  assert(h.gate.openFor() === null, "gate closed");
});

test("an intervening peer switch leaves the old rebind locked", () => {
  const h = build();
  h.gate.open(4);
  h.gate.refused(4);
  // The user switches again: generation 5 is live now.
  h.gate.acceptedSnapshot(5);
  assert(h.unlocks() === 0, "a snapshot for a newer switch must not unlock the old one");
  assert(h.gate.openFor() === 4, "the superseded rebind is still open, and stays unusable");
  // Only its own generation can settle it, and that origin is gone.
  h.gate.open(5);
  h.gate.acceptedSnapshot(5);
  assert(h.unlocks() === 1, "the new switch settles normally");
});

test("an outright failure closes without unlocking", () => {
  const h = build();
  h.gate.open(6);
  h.gate.failed(6);
  assert(h.unlocks() === 0, "a failure must not unlock");
  assert(h.announced[0]?.success === false, "failure is announced");
  assert(h.gate.openFor() === null, "gate closed on failure");
});

test("a stale settle for a closed generation does nothing", () => {
  const h = build();
  h.gate.open(7);
  h.gate.acceptedSnapshot(7);
  h.gate.acceptedSnapshot(7);
  assert(h.unlocks() === 1, "a second accepted snapshot must not unlock twice");
});

console.log(failures === 0 ? "\nall passed" : `\n${failures} failed`);
if (failures > 0) process.exitCode = 1;
