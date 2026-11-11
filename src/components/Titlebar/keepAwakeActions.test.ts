import assert from "node:assert/strict";
import { changeKeepAwake, type KeepAwakePorts } from "./keepAwakeActions";
import type { KeepAwakeStatus } from "../../../gen/KeepAwakeStatus";

const off: KeepAwakeStatus = { active: false, display: false, expires_at: null, gnome_policy: false, system_block: false };
const active = { ...off, active: true, expires_at: 12_001 };
let current = off;
const calls: unknown[] = [];
const ports: KeepAwakePorts = {
  status: async () => { calls.push("status"); return current; },
  start: async (seconds, display) => {
    calls.push({ seconds, display });
    current = { ...off, active: true, display, expires_at: seconds === undefined ? null : 10_000 + seconds * 1000 };
    return current;
  },
  stop: async () => { calls.push("stop"); current = off; },
  saveDisplay: async display => { calls.push({ saved: display }); },
  now: () => 10_000,
};

async function run() {
  const started = await changeKeepAwake({ kind: "start", seconds: 3600, display: true }, ports);
  assert.equal(started.status, current, "start returns authoritative state without an event");
  assert.deepEqual(calls.splice(0), [{ seconds: 3600, display: true }]);

  await changeKeepAwake({ kind: "start", seconds: null, display: false }, ports);
  assert.deepEqual(calls.splice(0), [{ seconds: undefined, display: false }]);

  const stopped = await changeKeepAwake({ kind: "stop" }, ports);
  assert.equal(stopped.status, off);
  assert.deepEqual(calls.splice(0), ["stop", "status"]);

  const preference = await changeKeepAwake({ kind: "display", display: true }, ports);
  assert.equal(preference.displayPreference, true);
  assert.deepEqual(calls.splice(0), ["status", { saved: true }], "an expired/externally stopped session is not restarted");

  current = active;
  await changeKeepAwake({ kind: "display", display: true }, ports);
  assert.deepEqual(calls.splice(0), ["status", { seconds: 3, display: true }, { saved: true }]);

  current = { ...active, expires_at: 9_999 };
  await changeKeepAwake({ kind: "display", display: false }, ports);
  assert.deepEqual(calls.splice(0), ["status", { seconds: 1, display: false }, { saved: false }]);

  current = { ...active, expires_at: null };
  await changeKeepAwake({ kind: "display", display: true }, ports);
  assert.deepEqual(calls.splice(0), ["status", { seconds: undefined, display: true }, { saved: true }]);

  const refusal = new Error("OS refused the inhibitor");
  for (const action of [
    { kind: "start", seconds: 3600, display: true },
    { kind: "stop" },
    { kind: "display", display: false },
  ] as const) {
    await assert.rejects(changeKeepAwake(action, {
      ...ports,
      start: async () => { throw refusal; },
      stop: async () => { throw refusal; },
    }), error => error === refusal);
  }
  current = off;
  await assert.rejects(changeKeepAwake({ kind: "display", display: true }, {
    ...ports, saveDisplay: async () => { throw refusal; },
  }), error => error === refusal);
  console.log("keepAwakeActions: 11 scenarios passed");
}

void run().catch(error => { console.error(error); process.exitCode = 1; });
