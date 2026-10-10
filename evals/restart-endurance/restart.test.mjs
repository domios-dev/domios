import test from "node:test";
import assert from "node:assert/strict";
import { launchRestart, restartObserved } from "./restart.mjs";

const before = { pid: 100, started_us: 1000 };

test("a fake launcher leaving the same listener cannot count as a restart", async () => {
  const calls = [];
  const evidence = await launchRestart({ snapshot: async () => before,
    stop: async () => { calls.push("stop"); }, start: async () => { calls.push("start"); },
  });
  assert.deepEqual(calls, ["stop", "start"]);
  assert.equal(evidence.pass, false);
  assert.equal(evidence.reason, "restart_not_observed");
  assert.deepEqual(evidence.before, evidence.after);
});

test("a new listener PID with a later start proves the stop/start", async () => {
  const snapshots = [before, { pid: 101, started_us: 1001 }];
  const evidence = await launchRestart({ snapshot: async () => snapshots.shift(), stop: async () => {}, start: async () => {} });
  assert.equal(evidence.pass, true);
});

test("missing start evidence, PID reuse, or an older different listener fail closed", () => {
  for (const after of [{ pid: 101 }, { pid: 100, started_us: 1001 }, { pid: 101, started_us: 999 }]) {
    assert.equal(restartObserved(before, after), false);
  }
});

test("a stop refusal preserves the pre-stop identity and never starts", async () => {
  const evidence = await launchRestart({ snapshot: async () => before,
    stop: async () => { throw new Error("refused"); }, start: async () => assert.fail("unsafe start"),
  });
  assert.equal(evidence.pass, false);
  assert.equal(evidence.phase, "stopping");
  assert.deepEqual(evidence.before, before);
});
