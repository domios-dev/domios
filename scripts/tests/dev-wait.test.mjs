import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readinessSignal, waitForReadiness } from "../dev-wait-health.mjs";

const node = process.execPath;
const fixture = (body) => {
  const dir = mkdtempSync(join(tmpdir(), "codeterm-dev-wait-health-"));
  const log = join(dir, "dev.log");
  const script = join(dir, "fixture.mjs");
  writeFileSync(log, "");
  writeFileSync(script, body);
  return { log, script };
};

test("dev_wait_health accepts an already-present explicit health signal", () => {
  const line = "REST API generation 3 health-proven on http://127.0.0.1:7686";
  assert.equal(readinessSignal(line, "7686"), true);
});

test("dev_wait_health observes eventual explicit health without polling", async () => {
  const { log, script } = fixture(`import { appendFileSync } from "node:fs"; setImmediate(() => appendFileSync(process.argv[2], "REST API generation 1 health-proven on http://127.0.0.1:7686\\n"));`);
  const result = await waitForReadiness({ log, apiPort: "7686", command: [node, script, log] });
  assert.match(result.signal, /health-proven/);
});

test("dev_wait_health reports failed readiness and never false-succeeds", async () => {
  const { log, script } = fixture("process.exit(17);");
  await assert.rejects(waitForReadiness({ log, apiPort: "7686", command: [node, script] }), /exited before health readiness.*code=17/s);
});

test("dev_wait_health scopes readiness to dev and rejects prod health", () => {
  const line = "REST API generation 1 health-proven on http://127.0.0.1:7685";
  assert.equal(readinessSignal(line, "7686"), false);
});
