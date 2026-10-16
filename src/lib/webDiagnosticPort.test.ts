import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { diagnosticBody, DiagnosticQueue, rejectionDiagnostic } from "./webDiagnosticPort";

const fixture = JSON.parse(readFileSync("tests/fixtures/frontend-diagnostic.json", "utf8"));
const flags = { native: false, injected: false, mounted: true };

test("web sender body matches shared Rust schema fixture and native sanitizer", () => {
  assert.deepEqual(JSON.parse(JSON.stringify(diagnosticBody(fixture.stage, fixture.failure, flags))), fixture.body);
  const script = readFileSync("src-tauri/src/startup/frontend_shell_bootstrap.js", "utf8")
    .replace("__CODETERM_SHELL_BOOTSTRAP__", JSON.stringify({ origin: "https://example.test", token: "opaque" }))
    .replace("__CODETERM_SHELL_DEBUG__", "true");
  const window = { top: undefined as unknown, location: { origin: "https://example.test" }, addEventListener() {}, __CODETERM_SHELL_DIAGNOSTIC__: undefined as unknown };
  window.top = window;
  const bodies: unknown[] = [];
  runInNewContext(script, { window, document: { documentElement: { dataset: { codetermMounted: "true" } } }, fetch: (_url: string, init: { body: string }) => { bodies.push(JSON.parse(init.body)); return Promise.resolve(); } });
  const send = window.__CODETERM_SHELL_DIAGNOSTIC__ as (stage: string, failure: object) => void;
  for (const name of ["Error", "ReferenceError", "TypeError", "SyntaxError", "SecurityError", "FrontendUpdateError", "PrivateError"]) {
    for (const reason of ["invalid-client", "checkpoint-unavailable", "status-unavailable", "api-incompatible", "storage-unavailable", "boot-context-invalid", "unexpected", "native-capability-denied", "rejected-string", "rejected-value", "secret"]) {
      const failure = { name, reason, httpStatus: 700, filename: "private path.js" };
      send("script-error", failure);
      assert.deepEqual(bodies[bodies.length - 1], JSON.parse(JSON.stringify(diagnosticBody("script-error", failure, { ...flags, injected: true }))));
    }
  }
});

test("bucket allows ten immediately and refills thirty per minute without timers", () => {
  const queue = new DiagnosticQueue(0);
  for (let i = 0; i < 12; i++) queue.push(fixture.body);
  for (let i = 0; i < 10; i++) assert.ok(queue.take(0));
  assert.equal(queue.take(1999), undefined);
  assert.ok(queue.take(2000));
  assert.equal(queue.take(2000), undefined);
  assert.ok(queue.take(4000));
});

test("queue drops oldest, caps at 32, and folds drops once into reason", () => {
  const queue = new DiagnosticQueue(0);
  for (let i = 0; i < 100; i++) queue.push({ ...fixture.body, line: i });
  const first = queue.take(0)!;
  assert.equal(first.line, 68);
  assert.equal(first.reason, "unexpected; dropped=68");
  assert.equal(queue.take(0)!.reason, "unexpected");
});

test("rejections expose only classified metadata", () => {
  assert.deepEqual(rejectionDiagnostic("plugin.call explicitly denied secret"), { reason: "native-capability-denied" });
  assert.deepEqual(rejectionDiagnostic("secret"), { reason: "rejected-string" });
  assert.deepEqual(rejectionDiagnostic(null), { name: undefined, reason: "rejected-value" });
});

test("oversize and cyclic records are dropped without breaking the queue", () => {
  const queue = new DiagnosticQueue(0);
  queue.push({ ...fixture.body, chat: "x".repeat(17 * 1024) });
  const cyclic: Record<string, unknown> = {};
  cyclic.self = cyclic;
  queue.push({ ...fixture.body, chat: cyclic });
  queue.push(fixture.body);
  assert.equal(queue.take(0)!.reason, "unexpected; dropped=2");
  assert.equal(queue.take(0), undefined);
});
