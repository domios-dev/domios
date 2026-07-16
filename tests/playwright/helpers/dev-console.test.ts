import assert from "node:assert/strict";
import net from "node:net";
import test from "node:test";
import {
  assertDevConsoleStopped,
  emitDevConsoleProgress,
  isWebUiDocument,
  nonAdoptableCdpError,
  windowsCleanupArgs,
} from "./dev-console";
import { devInstanceForSlot, type DevInstance } from "./dev-instance";
import { DEV_FIXTURE_TIMEOUT_MS } from "./fixtures";

function instance(apiPort: number): DevInstance {
  return {
    slot: 0,
    name: "pw-test",
    apiPort,
    vitePort: apiPort + 1,
    cdpPort: apiPort + 2,
    identifier: "playwright-pw-test",
    dataDirName: ".codeterm-dev-pw-test",
    apiBase: `http://127.0.0.1:${apiPort}`,
    cdpBase: `http://127.0.0.1:${apiPort + 2}`,
    webUrl: `http://127.0.0.1:${apiPort + 1}`,
  };
}

function listen(server: net.Server): Promise<number> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve((server.address() as net.AddressInfo).port));
  });
}

test("spawn guard refuses an occupied Playwright listener", async () => {
  const server = net.createServer();
  const port = await listen(server);
  try {
    await assert.rejects(() => assertDevConsoleStopped(instance(port), 1_000), /leaked listeners/);
  } finally {
    await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
  }
});

test("worker UI lifecycle has its own cold-start budget", () => {
  assert.equal(DEV_FIXTURE_TIMEOUT_MS, 660_000);
  assert.ok(DEV_FIXTURE_TIMEOUT_MS > 30_000);
});

test("windows cleanup plan is scoped to the instance and never targets prod", () => {
  const target = devInstanceForSlot(0, 2);
  const args = windowsCleanupArgs(target);
  const flag = (name: string) => args[args.indexOf(name) + 1];
  assert.equal(flag("-Instance"), target.name);
  assert.equal(flag("-Identifier"), target.identifier);
  assert.equal(flag("-ApiPort"), String(target.apiPort));
  assert.equal(flag("-VitePort"), String(target.vitePort));
  assert.equal(flag("-CdpPort"), String(target.cdpPort));
  assert.ok(args.some((arg) => arg.endsWith("windows-dev-cleanup.ps1")));
  assert.ok(!args.includes("7685") && !args.includes("7686"), "cleanup must never touch prod/dev");
});

test("healthy non-adoptable CDP diagnostics name the endpoint and remedy", () => {
  const error = nonAdoptableCdpError(instance(7900));
  assert.match(error.message, /lacks the CDP endpoint.*7902/);
  assert.match(error.message, /rerun via the Playwright harness/);
  assert.match(error.message, /WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS/);
});

test("progress emitter attributes each bootstrap phase", () => {
  const lines: string[] = [];
  const original = console.log;
  console.log = (line: string) => lines.push(line);
  try {
    emitDevConsoleProgress("health probe", "checking instance");
    emitDevConsoleProgress("kill verification", "listeners are free");
    emitDevConsoleProgress("dev --wait", "starting with deadline");
    emitDevConsoleProgress("post-start CDP readiness", "waiting for CDP");
  } finally {
    console.log = original;
  }
  assert.deepEqual(lines, [
    "[dev-console] health probe: checking instance",
    "[dev-console] kill verification: listeners are free",
    "[dev-console] dev --wait: starting with deadline",
    "[dev-console] post-start CDP readiness: waiting for CDP",
  ]);
});

test("web UI adoption accepts only the application document", () => {
  assert.equal(isWebUiDocument("text/html; charset=utf-8", '<div id="root"></div>'), true);
  assert.equal(isWebUiDocument("text/html", "<h1>Vite is starting</h1>"), false);
  assert.equal(isWebUiDocument("application/json", '<div id="root"></div>'), false);
});
