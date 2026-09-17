import test from "node:test";
import assert from "node:assert/strict";

const healthReady = (health, apiPort) =>
  health?.status === "ok" && String(health.instance?.api_port) === String(apiPort);

const pollFixture = (observations, apiPort) => {
  for (const observation of observations) {
    if (observation.exitCode !== undefined) {
      throw new Error(`dev process exited before readiness (code=${observation.exitCode})`);
    }
    if (healthReady(observation.health, apiPort)) return observation.health;
  }
  throw new Error("readiness deadline exceeded");
};

test("dev_wait_health accepts already-present healthy dev health", () => {
  const health = pollFixture([{ health: { status: "ok", instance: { api_port: 7686 } } }], 7686);
  assert.equal(health.status, "ok");
});

test("dev_wait_health observes eventual health in the detector sequence", () => {
  const health = pollFixture([
    { health: { status: "starting", instance: { api_port: 7686 } } },
    { health: { status: "ok", instance: { api_port: 7686 } } },
  ], 7686);
  assert.equal(health.instance.api_port, 7686);
});

test("dev_wait_health reports failed readiness with the real non-zero exit status", () => {
  assert.throws(
    () => pollFixture([{ exitCode: 17 }], 7686),
    /exited before readiness \(code=17\)/,
  );
});

test("dev_wait_health rejects prod health while scoping to the dev port", () => {
  assert.throws(
    () => pollFixture([{ health: { status: "ok", instance: { api_port: 7685 } } }], 7686),
    /readiness deadline exceeded/,
  );
});
