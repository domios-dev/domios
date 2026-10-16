import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Script } from "node:vm";

function fixture(platform = "linux") {
  let now = 1_000_000;
  let modified = 1;
  let starts = 0;
  let continuation: ((result: unknown) => unknown) | undefined;
  const host = {
    platform: () => platform,
    unixNowMs: () => now,
    envGet: () => null,
    expandHome: (value: string) => value,
    exec: () => { throw new Error("synchronous discovery is forbidden"); },
    execStart: () => { starts++; return JSON.stringify({ jobId: "catalogue" }); },
    awaitJob: (_id: string, then: typeof continuation) => { continuation = then; return { awaiting: true }; },
    fs: {
      readDir: () => { throw new Error("binary discovery must happen outside the VM"); },
      fileExists: () => false,
      readFile: () => null,
    },
  };
  const context = { host, module: { exports: {} as { default?: Record<string, (...args: unknown[]) => unknown> } } };
  new Script(readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
  const plugin = context.module.exports.default!;
  return {
    plugin,
    starts: () => starts,
    discover: () => plugin.discoverModels(JSON.stringify({ path: platform === "windows" ? "C:/bin/opencode.exe" : "/bin/opencode", len: 1, modified_unix: modified })),
    advance: (ms: number) => { now += ms; },
    replaceBinary: () => { modified++; },
    complete: (result = { code: 0, stdout: "fixture/model" }) => JSON.parse(JSON.stringify(continuation!(result))),
  };
}

test("discovery yields while cheap detection and cached Supporter reads remain callable", () => {
  const f = fixture();
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models("cold-fingerprint"))), { awaiting: true });
  assert.equal(f.starts(), 1);
  assert.equal(f.plugin.detectFromOutput("opaque"), false);
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models())), []);
  assert.equal(f.discover(), null);
  assert.equal(f.starts(), 1);
  const models = f.complete();
  assert.equal(models[0].id, "fixture/model");
  for (let i = 0; i < 30; i++) {
    assert.equal((f.plugin.models() as Array<{ id: string }>)[0].id, "fixture/model");
    assert.equal(f.plugin.selectModel("fixture/model"), true);
  }
  assert.equal(f.starts(), 1);
});

test("catalogue refresh honours the ten-minute TTL and binary mtime on both platforms", () => {
  for (const platform of ["linux", "windows"]) {
    const f = fixture(platform);
    f.discover();
    f.complete();
    f.advance(599_999);
    f.discover();
    assert.equal(f.starts(), 1);
    f.advance(1);
    f.discover();
    assert.equal(f.starts(), 2);
    f.complete();
    f.replaceBinary();
    f.discover();
    assert.equal(f.starts(), 3);
    f.complete();
  }
});

test("failed and abandoned discovery attempts do not create repeated subprocess bursts", () => {
  const f = fixture();
  f.discover();
  assert.equal(f.complete({ code: 1, stdout: "" }), null);
  f.discover();
  assert.equal(f.starts(), 1);
  f.advance(600_000);
  f.discover();
  assert.equal(f.starts(), 2);
  f.advance(600_000);
  f.discover();
  assert.equal(f.starts(), 3);
  f.complete();
});

test("Supporter retries a null cold catalogue after its TTL while a valid empty catalogue stays warm", () => {
  const f = fixture();
  f.plugin.models("cold-fingerprint");
  assert.deepEqual(f.complete({ code: 1, stdout: "" }), []);
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models("cold-fingerprint"))), []);
  assert.equal(f.starts(), 1);
  f.advance(600_000);
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models("cold-fingerprint"))), { awaiting: true });
  assert.equal(f.starts(), 2);
  assert.equal(f.complete()[0].id, "fixture/model");

  const empty = fixture();
  empty.plugin.models("empty-fingerprint");
  assert.deepEqual(empty.complete({ code: 0, stdout: 'fixture/unavailable\n{"available":false}' }), []);
  empty.advance(600_000);
  assert.deepEqual(JSON.parse(JSON.stringify(empty.plugin.models("empty-fingerprint"))), []);
  assert.equal(empty.starts(), 1);
});
