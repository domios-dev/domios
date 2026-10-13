import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createInterface } from "node:readline";
import test from "node:test";
import { fileURLToPath } from "node:url";

function peer(t, env = {}) {
  const child = spawn(process.execPath, [fileURLToPath(new URL("./fake-peer.mjs", import.meta.url))], {
    stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, ...env },
  });
  t.after(() => { if (child.exitCode === null) child.kill(); });
  const lines = createInterface({ input: child.stdout });
  return { child, lines, send: frame => child.stdin.write(`${JSON.stringify(frame)}\n`) };
}

test("fixture correlates numeric and string IDs without rewriting opaque input", { timeout: 5000 }, async t => {
  const { child, lines, send } = peer(t);
  const output = lines[Symbol.asyncIterator]();
  const body = { text: "`literal` $TOKEN\nsecond line", attachments: [] };
  send({ id: 1, method: "fixture/submit", params: body });
  send({ id: "1", method: "fixture/submit", params: body });
  for (const id of [1, "1"]) {
    assert.deepEqual(JSON.parse((await output.next()).value), { id, result: { method: "fixture/submit", params: body } });
  }
  child.stdin.end();
  assert.deepEqual(await once(child, "exit"), [0, null]);
});

test("fixture tool child inherits the selected tab environment", { timeout: 5000 }, async t => {
  const env = { CODETERM_TAB_ID: "own-tab", CODETERM_TASK_ID: "own-task", CODETERM_API_TOKEN: "opaque-fixture-token" };
  const { child, lines, send } = peer(t, env);
  send({ id: 1, method: "fixture/environment", params: { keys: Object.keys(env) } });
  assert.deepEqual(JSON.parse((await lines[Symbol.asyncIterator]().next()).value).result.environment, env);
  child.stdin.end();
  await once(child, "exit");
});

test("fixture exposes an exit status and stderr tail", { timeout: 5000 }, async t => {
  const { child, send } = peer(t);
  let stderr = "";
  child.stderr.on("data", chunk => { stderr += chunk; });
  const exited = once(child, "close");
  send({ id: 1, method: "fixture/exit", params: { code: 23 } });
  assert.deepEqual(await exited, [23, null]);
  assert.equal(stderr, "opaque fixture stderr");
});

test("fixture pipe EOF can occur while the process still owns stdin", { timeout: 5000 }, async t => {
  const { child, send } = peer(t);
  const ended = once(child.stdout, "end");
  child.stdout.resume();
  send({ id: 1, method: "fixture/drop", params: {} });
  await ended;
  assert.equal(child.exitCode, null);
  child.stdin.end();
  await once(child, "exit");
});
