import assert from "node:assert/strict";
import { test } from "node:test";
import { createSessionModelResource, usesPluginSessionModel } from "./shellSessionModelResource";

test("session model lookup waits for capabilities and excludes runtime-owned sessions", () => {
  const backing = { kind: "chat_backend" as const, plugin_id: "opaque-provider", session_id: "own-session" };
  assert.equal(usesPluginSessionModel(backing, undefined), false);
  assert.equal(usesPluginSessionModel(backing, { agent_runtime: {} }), false);
  assert.equal(usesPluginSessionModel(backing, {}), true);
  assert.equal(usesPluginSessionModel(undefined, {}), false);
});

test("failed lookups coalesce and remain cached across mux changes", async () => {
  let calls = 0;
  let clock = 0;
  const resource = createSessionModelResource(async () => { calls++; throw Error("unavailable"); }, () => clock);
  const pending = resource.load("provider", "session");
  assert.equal(resource.load("provider", "session"), pending);
  assert.equal(await pending, null);
  for (let event = 0; event < 100; event++) await resource.load("provider", "session");
  assert.equal(calls, 1);
  clock = 30_000;
  await resource.load("provider", "session");
  assert.equal(calls, 2);
  await resource.load("provider", "another-session");
  assert.equal(calls, 3);
});

test("refresh failure retains the own-session model without publishing a false change", async () => {
  let fail = false;
  let clock = 0;
  let changes = 0;
  const resource = createSessionModelResource(async () => { if (fail) throw Error("unavailable"); return "own-model"; }, () => clock);
  resource.subscribe(() => changes++);
  await resource.load("provider", "session");
  fail = true;
  clock = 30_000;
  assert.equal(await resource.load("provider", "session"), "own-model");
  assert.equal(resource.get("provider", "session"), "own-model");
  assert.equal(resource.get("provider", "other"), null);
  assert.equal(changes, 1);
});
