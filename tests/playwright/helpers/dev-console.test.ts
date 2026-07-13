import assert from "node:assert/strict";
import test from "node:test";
import { isWebUiDocument } from "./dev-console";
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
