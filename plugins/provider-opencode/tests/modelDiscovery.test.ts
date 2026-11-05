import { ioTestContext } from "../../provider-shared/tests/hostIo";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Script } from "node:vm";

function fixture(platform = "linux") {
  let now = 1_000_000;
  let modified = 1;
  let configuration = 1;
  let verboseResult: unknown;
  const continuations: Array<(result: unknown) => unknown> = [];
  let starts = 0;
  let verboseStarts = 0;
  let continuation: ((result: unknown) => unknown) | undefined;
  const host = {
    platform: () => platform,
    unixNowMs: () => now,
    envGet: () => null,
    expandHome: (value: string) => value,
    exec: () => { throw new Error("synchronous discovery is forbidden"); },
    execStart: (text: string) => {
      const options = JSON.parse(text);
      assert.equal(options.timeoutMs, 10000);
      if (options.args.includes("--verbose")) {
        verboseStarts++;
        return JSON.stringify({ jobId: "verbose" });
      }
      starts++;
      return JSON.stringify({ jobId: "plain" });
    },
    awaitJob: (id: string, then: (result: unknown) => unknown) => {
      if (id === "verbose") return then(verboseResult);
      continuation = then;
      continuations.push(then);
      return { awaiting: true };
    },
    fs: {
      readDir: () => { throw new Error("binary discovery must happen outside the VM"); },
      fileExists: () => false,
      readFile: () => null,
    },
  };
  const context = { host, module: { exports: {} as { default?: Record<string, (...args: unknown[]) => unknown> } } };
  new Script(readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(ioTestContext(context));
  const plugin = context.module.exports.default!;
  return {
    plugin,
    starts: () => starts,
    verboseStarts: () => verboseStarts,
    fingerprint: () => JSON.stringify({ path: platform === "windows" ? "C:/bin/opencode.exe" : "/bin/opencode", len: 1, modified_unix: modified, configuration }),
    discover: () => plugin.discoverModels(JSON.stringify({ path: platform === "windows" ? "C:/bin/opencode.exe" : "/bin/opencode", len: 1, modified_unix: modified, configuration })),
    advance: (ms: number) => { now += ms; },
    replaceBinary: () => { modified++; },
    changeConfiguration: () => { configuration++; },
    // Resolves the deferred plain listing with `plain`; the verbose enrichment then answers `result`.
    complete: (result: unknown = { code: 0, stdout: "fixture/model" }, index?: number, plain: unknown = result) => {
      verboseResult = result;
      return JSON.parse(JSON.stringify((index === undefined ? continuation! : continuations[index])(plain)));
    },
  };
}

test("discovery yields while cheap detection and cached Supporter reads remain callable", () => {
  const f = fixture();
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models(f.fingerprint()))), { awaiting: true });
  assert.equal(f.starts(), 1);
  assert.equal(f.plugin.detectFromOutput("opaque"), false);
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models())), []);
  assert.equal(f.discover(), null);
  assert.equal(f.starts(), 1);
  const models = f.complete();
  assert.equal(models[0].id, "fixture/model");
  assert.equal(f.verboseStarts(), 1);
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

test("a failed attempt is never cached and an abandoned one blocks retries only within the pending window", () => {
  const f = fixture();
  f.discover();
  assert.equal(f.complete({ code: 1, stdout: "" }), null);
  assert.deepEqual(JSON.parse(JSON.stringify(f.discover())), { awaiting: true });
  assert.equal(f.starts(), 2);
  f.advance(29_999);
  assert.equal(f.discover(), null);
  assert.equal(f.starts(), 2);
  f.advance(1);
  f.discover();
  assert.equal(f.starts(), 3);
  assert.equal(f.complete()[0].id, "fixture/model");
});

test("a truncated verbose listing keeps the plain identities from one plain exec", () => {
  const f = fixture();
  f.discover();
  const models = f.complete({ code: 0, stdout: "endpoint/partial", truncated: true }, undefined, { code: 0, stdout: "endpoint/first\nendpoint/plain" });
  assert.deepEqual(models.map((model: { id: string }) => model.id), ["endpoint/first", "endpoint/plain"]);
  assert.equal(f.starts(), 1);
  assert.equal(f.verboseStarts(), 1);
  f.discover();
  assert.equal(f.starts(), 1);
});

test("Supporter retries a failed cold catalogue at once while a valid empty catalogue stays warm", () => {
  const f = fixture();
  f.plugin.models(f.fingerprint());
  assert.deepEqual(f.complete({ code: 1, stdout: "" }), []);
  assert.equal(f.starts(), 1);
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models(f.fingerprint()))), { awaiting: true });
  assert.equal(f.starts(), 2);
  assert.equal(f.complete()[0].id, "fixture/model");

  const empty = fixture();
  empty.plugin.models("empty-fingerprint");
  assert.deepEqual(empty.complete({ code: 0, stdout: 'fixture/unavailable\n{"available":false}' }), []);
  empty.advance(600_000);
  assert.deepEqual(JSON.parse(JSON.stringify(empty.plugin.models("empty-fingerprint"))), []);
  assert.equal(empty.starts(), 1);
});

test("add update remove and account changes immediately rediscover at the same clock and binary", () => {
  const f = fixture();
  f.discover();
  assert.deepEqual(f.complete().map((model: { id: string }) => model.id), ["fixture/model"]);
  for (const stdout of ["fixture/model\nendpoint/latest", "fixture/model\nendpoint/replacement", "fixture/model", "native/account-model"]) {
    f.changeConfiguration();
    assert.deepEqual(JSON.parse(JSON.stringify(f.discover())), { awaiting: true });
    assert.deepEqual(f.complete({ code: 0, stdout }).map((model: { id: string }) => model.id), stdout.split("\n"));
    const starts = f.starts();
    f.discover();
    assert.equal(f.starts(), starts);
  }
});

test("a late old-configuration job cannot overwrite or masquerade as a new discovery", () => {
  const f = fixture();
  f.discover();
  f.changeConfiguration();
  f.discover();
  assert.equal(f.complete({ code: 0, stdout: "endpoint/new" }, 1)[0].id, "endpoint/new");
  assert.equal(f.complete({ code: 0, stdout: "endpoint/old" }, 0), null);
  assert.equal((f.plugin.models() as Array<{id: string}>)[0].id, "endpoint/new");
  assert.equal((f.discover() as Array<{id: string}>)[0].id, "endpoint/new");
});

test("failed new-input discovery preserves cheap last-good reads without advertising a fresh result", () => {
  const f = fixture();
  f.discover(); f.complete();
  f.changeConfiguration(); f.discover();
  assert.equal(f.complete({ code: 1, stdout: "" }), null);
  assert.equal((f.plugin.models() as Array<{id: string}>)[0].id, "fixture/model");
  assert.deepEqual(JSON.parse(JSON.stringify(f.discover())), { awaiting: true });
  assert.equal((f.plugin.models() as Array<{id: string}>)[0].id, "fixture/model");
});

test("a complete verbose listing enriches the plain list; a failed or unparseable one keeps the plain list", () => {
  const ids = (models: Array<{id: string}>) => models.map((model) => model.id);
  const f = fixture();
  f.discover();
  const models = f.complete({ code: 0, stdout: 'endpoint/first\n{"variants":{"high":{}}}\nendpoint/latest\nendpoint/gone\n{"status":"deprecated"}\n' });
  assert.deepEqual(ids(models), ["endpoint/first", "endpoint/latest"]);
  assert.deepEqual(models[0].reasoningEfforts.map((effort: {id: string}) => effort.id), ["default", "high"]);
  assert.equal(f.starts(), 1);
  for (const verbose of [
    { code: 0, stdout: "endpoint/partial", truncated: true },
    { code: 0, stdout: "endpoint/failure", error: "timeout" },
    { code: 0, stdout: "no model identities" },
  ]) {
    f.changeConfiguration(); f.discover();
    assert.deepEqual(ids(f.complete(verbose, undefined, { code: 0, stdout: "endpoint/first\nendpoint/plain" })), ["endpoint/first", "endpoint/plain"]);
  }
  assert.equal(f.starts(), 4);
  assert.equal(f.verboseStarts(), 4);
  f.changeConfiguration(); f.discover();
  assert.equal(f.complete({ code: 1, stdout: "" }, undefined, { code: 0, stdout: "endpoint/partial", truncated: true }), null);
});
