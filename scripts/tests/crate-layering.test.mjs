import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const script = path.resolve(import.meta.dirname, "../check-crate-layering.sh");
const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "crate-layering-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const write = (name, file, text) => {
    const target = path.join(root, "src-tauri/crates", name, file);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, text);
  };
  for (const name of ["codeterm-events", "codeterm-keepawake", "codeterm-surface"]) write(name, "Cargo.toml", '[package]\nname = "' + name + '"');
  write("codeterm-prompts", "Cargo.toml", '[package]\nname = "codeterm-prompts"');
  return { write, run: () => spawnSync(bash, [script, root], { encoding: "utf8" }) };
}

test("allows lower dependencies and host adapters", (t) => {
  const { write, run } = fixture(t);
  write("codeterm-api", "Cargo.toml", 'codeterm-service-core = { path = "../codeterm-service-core" }\ntauri = "2"');
  write("codeterm-prompts", "src/lib.rs", 'pub use codeterm_types::agent::AgentRole;');
  assert.equal(run().status, 0);
});
for (const [name, file, text] of [
  ["codeterm-prompts", "Cargo.toml", 'alias = { package = "tauri", version = "2" }'],
  ["codeterm-prompts", "Cargo.toml", 'codeterm-service-core = "0.0.0"'],
  ["codeterm-prompts", "src/lib.rs", 'use tauri::Runtime;'],
  ["codeterm-prompts", "build.rs", 'extern crate tauri;'],
  ["codeterm-prompts", "tests/nested/leak.rs", 'use codeterm_service_core::service::ServiceContext;'],
  ["codeterm-events", "src/lib.rs", 'use codeterm_service_core::ServiceContext;'],
  ["codeterm-surface", "src/nested/leak.rs", 'struct ServiceContext;'],
]) {
  test(`rejects ${name}/${file}: ${text}`, (t) => {
    const { write, run } = fixture(t);
    write(name, file, text);
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /leaf crate names/);
  });
}
for (const name of ["codeterm-events", "codeterm-keepawake", "codeterm-surface"]) {
  test(`rejects framework dependency in ${name}`, (t) => {
    const { write, run } = fixture(t);
    write(name, "Cargo.toml", 'tauri = "2"');
    const result = run();
    assert.equal(result.status, 1);
    assert.match(result.stderr, /leaf crate names tauri/);
  });
}

test("allows same and lower leaf tiers, including supporter consumers", (t) => {
  const { write, run } = fixture(t);
  for (const [name, dependency] of [
    ["codeterm-agent-model", "codeterm-config"],
    ["codeterm-supporter", "codeterm-agent-model"],
    ["codeterm-recall", "codeterm-supporter"],
    ["codeterm-provider-ops", "codeterm-supporter"],
    ["codeterm-events", "codeterm-config"],
    ["codeterm-surface", "codeterm-provider-ops"],
    ["codeterm-surface", "codeterm-browser"],
  ]) {
    write(name, "Cargo.toml", `[package]\nname = "${name}"\n[dependencies]\n${dependency} = { path = "../${dependency}" }\n`);
  }
  const result = run();
  assert.equal(result.status, 0, result.stderr);
});

for (const declaration of [
  '[dependencies]\ncodeterm-supporter = "0.0.0"',
  '[dev-dependencies]\nrenamed = { package = "codeterm-supporter", path = "../codeterm-supporter" }',
  '[build-dependencies.renamed]\npackage = "codeterm-supporter"\npath = "../codeterm-supporter"',
  '[target.\'cfg(windows)\'.dependencies]\nrenamed = {\npackage = "codeterm-supporter",\npath = "../codeterm-supporter"\n}',
]) {
  test(`rejects reversed agent-model edge: ${declaration}`, (t) => {
    const { write, run } = fixture(t);
    write("codeterm-agent-model", "Cargo.toml", `[package]\nname = "codeterm-agent-model"\n${declaration}\n`);
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /leaf tier inversion: codeterm-agent-model .* -> codeterm-supporter/);
  });
}

for (const [name, dependency] of [
  ["codeterm-supporter", "codeterm-recall"],
  ["codeterm-supporter", "codeterm-provider-ops"],
  ["codeterm-types", "codeterm-agent-model"],
]) {
  test(`rejects reversed ${name} -> ${dependency} edge`, (t) => {
    const { write, run } = fixture(t);
    write(name, "Cargo.toml", `[package]\nname = "${name}"\n[dependencies]\n${dependency} = { path = "../${dependency}" }\n`);
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /leaf tier inversion/);
  });
}

for (const name of ["codeterm-service-core", "codeterm-events"]) {
  test(`rejects host runtime dependency in ${name}`, (t) => {
    const { write, run } = fixture(t);
    write(name, "Cargo.toml", '[dependencies]\ncodeterm-host-runtime = { path = "../codeterm-host-runtime" }\n');
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /core or leaf crate depends on codeterm-host-runtime/);
  });
}

test("allows the host runtime to own Tauri adapters", (t) => {
  const { write, run } = fixture(t);
  write("codeterm-host-runtime", "Cargo.toml", '[dependencies]\ntauri = "2"\ncodeterm-events = { path = "../codeterm-events" }\n');
  write("codeterm-host-runtime", "src/lib.rs", 'pub type Runtime = tauri::Wry;');
  const result = run();
  assert.equal(result.status, 0, result.stderr);
});

for (const declaration of [
  '[dependencies]\ntauri = "2"',
  '[dev-dependencies]\nrenamed = { package = "tauri", version = "2" }',
  '[target.\'cfg(windows)\'.dependencies]\ntauri-plugin-shell = "2"',
]) {
  test(`rejects service-core framework dependency: ${declaration}`, (t) => {
    const { write, run } = fixture(t);
    write("codeterm-service-core", "Cargo.toml", declaration);
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /service core depends on tauri/);
  });
}
