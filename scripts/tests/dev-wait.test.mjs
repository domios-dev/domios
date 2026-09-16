import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readinessSignal, waitForReadiness } from "../dev-wait-health.mjs";

const fixture = () => {
  const dir = mkdtempSync(join(tmpdir(), "codeterm dev wait health-"));
  const log = join(dir, "dev.log");
  const exitFile = join(dir, "dev.exit");
  writeFileSync(log, "");
  return { log, exitFile };
};

test("dev_wait_health accepts an already-present explicit health signal", async () => {
  const line = "REST API generation 3 health-proven on http://127.0.0.1:7686";
  const { log, exitFile } = fixture();
  writeFileSync(log, `${line}\n`);
  const result = await waitForReadiness({ log, exitFile, apiPort: "7686" });
  assert.equal(readinessSignal(result.signal, "7686"), true);
});

test("dev_wait_health observes eventual explicit health without polling", async () => {
  const { log, exitFile } = fixture();
  queueMicrotask(() => writeFileSync(log, "REST API generation 1 health-proven on http://127.0.0.1:7686\n"));
  const result = await waitForReadiness({ log, exitFile, apiPort: "7686" });
  assert.match(result.signal, /health-proven/);
});

test("dev_wait_health reports failed readiness and never false-succeeds", async () => {
  const { log, exitFile } = fixture();
  queueMicrotask(() => writeFileSync(exitFile, '{"code":17,"signal":null}\n'));
  await assert.rejects(waitForReadiness({ log, exitFile, apiPort: "7686" }), /exited before health readiness.*code=17/s);
});

test("dev_wait_health scopes readiness to dev and rejects prod health", () => {
  const line = "REST API generation 1 health-proven on http://127.0.0.1:7685";
  assert.equal(readinessSignal(line, "7686"), false);
});
