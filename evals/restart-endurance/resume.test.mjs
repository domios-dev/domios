import test from "node:test";
import assert from "node:assert/strict";
import { resumeSetup } from "./resume.mjs";

test("explicit pre-launch continuation preserves the harness failure and all seed evidence", () => {
  const seed = { pass: true, answer: "marker" };
  const result = { setup_complete: true, rounds: [{ round: 0, rows: [seed] }], fatal: "busy", round_started_at: "before" };
  assert.equal(resumeSetup(result, false, "resume"), result);
  assert.equal(result.rounds[0].rows[0], seed);
  assert.equal(result.fatal, undefined);
  assert.deepEqual(result.harness_failures, [{ reason: "busy", resumed_at: "resume", before_round: "before", pre_restart_tabs: undefined }]);
});

test("continuation refuses incomplete setup, a launcher attempt, or an existing restart round", () => {
  for (const [result, launched] of [
    [{ setup_complete: false, rounds: [] }, false],
    [{ setup_complete: true, rounds: [] }, true],
    [{ setup_complete: true, rounds: [{ round: 1 }] }, false],
  ]) assert.throws(() => resumeSetup(result, launched, "resume"));
});
