import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { assertTarget } from "./target.mjs";

const cli = fileURLToPath(new URL("./target.mjs", import.meta.url));
test("both approved named instance/port pairs pass", () => {
  for (const [instance, port] of [["aiderqa", 7697], ["endureqa", 7699]]) {
    assert.deepEqual(assertTarget(instance, String(port)), { instance, port });
    assert.equal(spawnSync(process.execPath, [cli, instance, String(port)]).status, 0);
  }
});

test("default DEV, PROD and unknown names or ports are refused", () => {
  for (const [instance, port] of [["", 7686], ["", 7685], ["aiderqa", 7685], ["endureqa", 7686], ["other", 7699], ["endureqa", 7700], ["", 7699]]) {
    assert.throws(() => assertTarget(instance, port));
    assert.notEqual(spawnSync(process.execPath, [cli, instance, String(port)]).status, 0);
  }
});

test("swapped instance/port pairs never redirect to another named DEV", () => {
  assert.throws(() => assertTarget("aiderqa", 7699));
  assert.throws(() => assertTarget("endureqa", 7697));
});
