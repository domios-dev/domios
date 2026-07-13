/**
 * WHAT: Playwright worker slots resolve to isolated CodeTerm dev instances.
 * WHY: Parallel workers must never share API, UI, token, or mutable state.
 * ORACLE: Each slot has the expected pwN identity and disjoint ports/data dirs.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { currentDevInstance, devInstanceForSlot } from "./dev-instance";

const ENV_KEYS = [
  "CT_E2E_WORKERS",
  "CT_E2E_PORT",
  "CT_E2E_PORT_BASE",
  "CT_E2E_INSTANCE",
  "CT_E2E_DATADIR",
  "TEST_PARALLEL_INDEX",
] as const;

function withEnv(values: Partial<Record<(typeof ENV_KEYS)[number], string>>, run: () => void): void {
  const saved = Object.fromEntries(ENV_KEYS.map((key) => [key, process.env[key]]));
  try {
    for (const key of ENV_KEYS) delete process.env[key];
    Object.assign(process.env, values);
    run();
  } finally {
    for (const key of ENV_KEYS) {
      const value = saved[key];
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
}

test("serial defaults use the isolated Playwright instance", () => {
  withEnv({}, () => {
    const instance = devInstanceForSlot(0);
    assert.equal(instance.name, "playwright");
    assert.equal(instance.apiPort, 7786);
    assert.equal(instance.dataDirName, ".codeterm-dev-playwright");
  });
});

test("an explicit serial instance receives isolated UI ports", () => {
  withEnv({ CT_E2E_INSTANCE: "pwserial", CT_E2E_PORT: "7790" }, () => {
    const instance = devInstanceForSlot(0);
    assert.equal(instance.name, "pwserial");
    assert.equal(instance.apiPort, 7790);
    assert.notEqual(instance.vitePort, 1420);
    assert.notEqual(instance.cdpPort, 9223);
  });
});

test("two worker slots have disjoint named instances", () => {
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    const first = devInstanceForSlot(0);
    const second = devInstanceForSlot(1);
    assert.deepEqual(
      [first.name, second.name],
      ["pw1", "pw2"],
    );
    assert.equal(first.apiPort, 7786);
    assert.equal(second.apiPort, 7787);
    assert.notEqual(first.vitePort, second.vitePort);
    assert.notEqual(first.cdpPort, second.cdpPort);
    assert.notEqual(first.dataDirName, second.dataDirName);
  });
});

test("parallelIndex pins the worker process to its instance slot", () => {
  withEnv({ CT_E2E_WORKERS: "2", TEST_PARALLEL_INDEX: "1" }, () => {
    const instance = currentDevInstance();
    assert.equal(instance.slot, 1);
    assert.equal(instance.name, "pw2");
  });
});

test("prod and out-of-range slots are rejected", () => {
  withEnv({ CT_E2E_PORT: "7685" }, () => {
    assert.throws(() => devInstanceForSlot(0), /unsafe Playwright instance port 7685/);
  });
  withEnv({ CT_E2E_PORT: "7686" }, () => {
    assert.throws(() => devInstanceForSlot(0), /unsafe Playwright instance port 7686/);
  });
  withEnv({ CT_E2E_WORKERS: "2" }, () => {
    assert.throws(() => devInstanceForSlot(2), /outside 0\.\.1/);
  });
});
