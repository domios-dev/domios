// Caller: CI tests-fast.yml guards via tests/catalogue.json node-script-tests
import assert from "node:assert/strict";
import test from "node:test";
import { planPause } from "./runner-toggle.mjs";

const runner = (id, name, os, labels, status = "online") => ({
  id,
  name,
  status,
  labels: [{ name: "self-hosted", type: "read-only" }, { name: os, type: "read-only" }, ...labels.map((name) => ({ name, type: "custom" }))],
});

test("pausing parks the machine's runners and lends labels nobody else serves", () => {
  const runners = [
    runner(1, "BLACKSTAR-windows-x64", "Windows", ["windows-x64-build", "windows-ci"]),
    runner(2, "BLACKSTAR-wsl-ci", "Linux", ["linux-ci", "light"]),
    runner(3, "ROG-windows-build", "Windows", ["windows-build", "windows-ci"]),
    runner(4, "ubunta-linux-build", "Linux", ["linux-ci", "light"]),
  ];
  const plan = planPause(runners, "BLACKSTAR");
  assert.deepEqual(plan.runners.map((r) => r.name), ["BLACKSTAR-windows-x64", "BLACKSTAR-wsl-ci"]);
  assert.deepEqual(plan.runners[0].labels, ["windows-x64-build", "windows-ci"]);
  assert.deepEqual(plan.borrowed, { "ROG-windows-build": ["windows-x64-build"] });
});

test("a label with no same-OS online runner refuses to pause", () => {
  const runners = [
    runner(1, "BLACKSTAR-windows-x64", "Windows", ["windows-x64-build"]),
    runner(2, "ROG-windows-build", "Windows", ["windows-build"], "offline"),
  ];
  assert.throws(() => planPause(runners, "BLACKSTAR"), /windows-x64-build/);
});

test("an unknown machine refuses to pause", () => {
  assert.throws(() => planPause([runner(1, "ROG-windows-build", "Windows", [])], "BLACKSTAR"), /no runner/);
});

test("a host label is saved for resume but never lent to another machine", () => {
  const runners = [
    runner(1, "BLACKSTAR-windows-x64", "Windows", ["windows-ci", "host-blackstar"]),
    runner(2, "ROG-windows-build", "Windows", ["windows-ci", "host-rog"]),
  ];
  const plan = planPause(runners, "BLACKSTAR");
  assert.deepEqual(plan.runners[0].labels, ["windows-ci", "host-blackstar"]);
  assert.deepEqual(plan.borrowed, {});
});
