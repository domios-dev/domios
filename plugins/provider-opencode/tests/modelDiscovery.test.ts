import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Script } from "node:vm";

function fixture(platform = "linux") {
  let now = 1_000_000;
  let modified = 1;
  let configuration = 1;
  let metadataResult: unknown;
  const continuations: Array<(result: unknown) => unknown> = [];
  let starts = 0;
  let continuation: ((result: unknown) => unknown) | undefined;
  const host = {
    platform: () => platform,
    unixNowMs: () => now,
    envGet: () => null,
    expandHome: (value: string) => value,
    exec: () => { throw new Error("synchronous discovery is forbidden"); },
    execStart: (text: string) => {
      const options = JSON.parse(text);
      assert.equal(options.timeoutMs, 3000);
      if (options.args.includes("--verbose")) return JSON.stringify({ jobId: "metadata" });
      starts++;
      return JSON.stringify({ jobId: "catalogue" });
    },
    awaitJob: (id: string, then: (result: unknown) => unknown) => {
      if (id === "metadata") return then(metadataResult);
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
  new Script(readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
  const plugin = context.module.exports.default!;
  return {
    plugin,
    starts: () => starts,
    fingerprint: () => JSON.stringify({ path: platform === "windows" ? "C:/bin/opencode.exe" : "/bin/opencode", len: 1, modified_unix: modified, configuration }),
    discover: () => plugin.discoverModels(JSON.stringify({ path: platform === "windows" ? "C:/bin/opencode.exe" : "/bin/opencode", len: 1, modified_unix: modified, configuration })),
    advance: (ms: number) => { now += ms; },
    replaceBinary: () => { modified++; },
    changeConfiguration: () => { configuration++; },
    complete: (result: unknown = { code: 0, stdout: "fixture/model" }, index?: number, metadata: unknown = result) => {
      metadataResult = metadata;
      return JSON.parse(JSON.stringify((index === undefined ? continuation! : continuations[index])(result)));
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
  f.plugin.models(f.fingerprint());
  assert.deepEqual(f.complete({ code: 1, stdout: "" }), []);
  assert.deepEqual(JSON.parse(JSON.stringify(f.plugin.models(f.fingerprint()))), []);
  assert.equal(f.starts(), 1);
  f.advance(600_000);
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
  assert.equal(f.discover(), null);
  assert.equal((f.plugin.models() as Array<{id: string}>)[0].id, "fixture/model");
});

test("only a complete identity listing is authoritative while verbose metadata may be partial", () => {
  const f = fixture();
  f.discover();
  assert.equal(f.complete({ code: 0, stdout: "endpoint/partial", truncated: true }), null);
  f.changeConfiguration(); f.discover();
  const models = f.complete(
    { code: 0, stdout: "endpoint/first\nendpoint/latest" },
    undefined,
    { code: 0, stdout: 'endpoint/first\n{"variants":{"high":{}}}\n', truncated: true },
  );
  assert.deepEqual(models.map((model: {id: string}) => model.id), ["endpoint/first", "endpoint/latest"]);
  assert.deepEqual(models[0].reasoningEfforts.map((effort: {id: string}) => effort.id), ["default", "high"]);
  f.changeConfiguration(); f.discover();
  assert.equal(f.complete({ code: 0, stdout: "endpoint/failure", error: "timeout" }), null);
});
