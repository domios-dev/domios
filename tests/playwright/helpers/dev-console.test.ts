import assert from "node:assert/strict";
import test from "node:test";
import { isWebUiDocument, windowsCleanupArgs } from "./dev-console";
import { devInstanceForSlot } from "./dev-instance";
import { DEV_FIXTURE_TIMEOUT_MS } from "./fixtures";

test("web UI readiness requires the application HTML shell", () => {
  assert.equal(isWebUiDocument("text/html; charset=utf-8", '<div id="root"></div>'), true);
  assert.equal(isWebUiDocument("text/html", "<h1>Vite is starting</h1>"), false);
  assert.equal(isWebUiDocument("application/json", '<div id="root"></div>'), false);
});

test("worker UI lifecycle has its own cold-start budget", () => {
  assert.equal(DEV_FIXTURE_TIMEOUT_MS, 660_000);
  assert.ok(DEV_FIXTURE_TIMEOUT_MS > 30_000);
});

test("windows cleanup plan is scoped to the instance and never targets prod", () => {
  const instance = devInstanceForSlot(0, 2);
  const args = windowsCleanupArgs(instance);
  const flag = (name: string) => args[args.indexOf(name) + 1];
  assert.equal(flag("-Instance"), instance.name);
  assert.equal(flag("-Identifier"), instance.identifier);
  assert.equal(flag("-ApiPort"), String(instance.apiPort));
  assert.equal(flag("-VitePort"), String(instance.vitePort));
  assert.equal(flag("-CdpPort"), String(instance.cdpPort));
  assert.ok(args.some((a) => a.endsWith("windows-dev-cleanup.ps1")));
  assert.ok(!args.includes("7685") && !args.includes("7686"), "cleanup must never touch prod/dev");
});
