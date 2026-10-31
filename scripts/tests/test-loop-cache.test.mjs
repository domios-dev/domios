import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { prepareNextestMetadata } from "../nextest-metadata-cache.mjs";
import { cachedPreparation } from "../preparation-cache.mjs";

test("nextest metadata reuse requires Cargo Fresh and unchanged binaries and manifests", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "nextest-cache-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const manifest = path.join(root, "Cargo.toml"), binary = path.join(root, "test-bin");
  fs.writeFileSync(manifest, "opaque manifest");
  fs.writeFileSync(binary, "opaque binary");
  const calls = [], logs = [];
  const compileFailure = Object.assign(new Error("compile refused"), { status: 43,
    stdout: [
      { reason: "compiler-message", message: { level: "error", rendered: "error[E0001]: opaque compile failure\n  --> owned.rs:7:3\n" } },
      { reason: "compiler-message", message: { level: "warning", message: "opaque compiler warning" } },
      { reason: "compiler-artifact", opaque: "must not dump artifact JSON" },
    ].map(row => JSON.stringify(row)).join("\n") + "\nnot cargo JSON\n" });
  let fresh = true, fail = false;
  const run = (program, args) => {
    if (program === "git") return "Cargo.toml\0";
    assert.ok(args.includes("--locked"));
    calls.push(args[0]);
    if (fail) throw compileFailure;
    if (args[0] === "test") return JSON.stringify({ reason: "compiler-artifact", fresh, profile: { test: true }, executable: binary }) + "\n";
    if (args[0] === "nextest") return JSON.stringify({ "rust-binaries": { opaque: { "binary-path": binary } } });
    if (args[0] === "metadata") return JSON.stringify({ packages: [{ manifest_path: manifest }] });
    throw new Error(args.join(" "));
  };
  const options = { root, manifest: "Cargo.toml", targets: ["--lib"], output: path.join(root, "out"), run, env: {}, log: line => logs.push(line) };
  assert.equal(prepareNextestMetadata(options), false);
  assert.deepEqual(calls.splice(0), ["nextest", "metadata"]);
  assert.equal(prepareNextestMetadata(options), true);
  assert.deepEqual(calls.splice(0), ["test"]);
  fresh = false;
  assert.equal(prepareNextestMetadata(options), false);
  assert.deepEqual(calls.splice(0), ["test", "nextest", "metadata"]);
  fresh = true;
  fs.writeFileSync(binary, "changed binary bytes");
  assert.equal(prepareNextestMetadata(options), false);
  assert.deepEqual(calls.splice(0), ["test", "nextest", "metadata"]);
  fs.writeFileSync(manifest, "changed dependency graph");
  assert.equal(prepareNextestMetadata(options), false);
  assert.deepEqual(calls.splice(0), ["nextest", "metadata"]);
  fail = true;
  assert.throws(() => prepareNextestMetadata(options), /compile refused/);
  assert.deepEqual(calls, ["test"]);
  assert.ok(logs.includes("error[E0001]: opaque compile failure\n  --> owned.rs:7:3"));
  assert.ok(logs.includes("opaque compiler warning"));
  assert.ok(logs.every(line => !line.includes("must not dump artifact JSON") && !line.includes("not cargo JSON")));
  assert.equal(compileFailure.status, 43);
});

test("preparation stamps skip unchanged work and repair removed outputs", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "prep-cache-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const input = path.join(root, "config"), output = path.join(root, "output");
  fs.writeFileSync(input, "opaque config");
  let calls = 0;
  const prepare = () => { calls++; fs.writeFileSync(output, "prepared"); };
  const run = () => cachedPreparation(root, "fixture", [input, output], prepare);
  assert.equal(run(), false);
  assert.equal(run(), true);
  assert.equal(calls, 1);
  fs.rmSync(output);
  assert.equal(run(), false);
  assert.equal(calls, 2);
  fs.writeFileSync(input, "changed config");
  assert.equal(run(), false);
  assert.equal(calls, 3);
  assert.throws(() => cachedPreparation(root, "failed", [input], () => { throw new Error("failure"); }), /failure/);
  assert.equal(fs.existsSync(path.join(root, "target/test-preparation/failed.json")), false);
});
