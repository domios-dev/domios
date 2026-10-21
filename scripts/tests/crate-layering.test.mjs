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
  for (const name of ["codeterm-events", "codeterm-keepawake", "codeterm-crash", "codeterm-surface"]) write(name, "Cargo.toml", '[package]\nname = "' + name + '"');
  write("codeterm-prompts", "Cargo.toml", '[package]\nname = "codeterm-prompts"');
  const writeRoot = (text) => fs.writeFileSync(path.join(root, "src-tauri/Cargo.toml"), text);
  return { write, writeRoot, run: () => spawnSync(bash, [script, root], { encoding: "utf8" }) };
}

test("allows lower dependencies and host adapters", (t) => {
  const { write, run } = fixture(t);
  write("codeterm-api", "Cargo.toml", 'codeterm-service-core = { path = "../codeterm-service-core" }\ntauri = "2"');
  write("codeterm-prompts", "src/lib.rs", 'pub use codeterm_types::agent::AgentRole;');
  assert.equal(run().status, 0);
});

test("Cargo path values accept the src-tauri directory without admitting framework dependencies", (t) => {
  const { write, run } = fixture(t);
  const sourcePath = 'path = "../../src-tauri/crates/codeterm-paths"';
  write("codeterm-events", "Cargo.toml", `[dependencies]\ncodeterm-paths = { ${sourcePath} }\n`);
  let result = run();
  assert.equal(result.status, 0, result.stderr);
  for (const declaration of [
    `[dependencies]\ntauri = { ${sourcePath} }`,
    `[dependencies]\nrenamed = { package = "tauri", ${sourcePath} }`,
    `[dev-dependencies.tauri]\n${sourcePath}`,
    `[dependencies.renamed]\npackage = "codeterm-service-core"\n${sourcePath}`,
  ]) {
    write("codeterm-events", "Cargo.toml", declaration);
    result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /leaf crate names (?:tauri|codeterm-service-core)/);
  }
});
for (const [name, file, text] of [
  ["codeterm-prompts", "Cargo.toml", 'alias = { package = "tauri", version = "2" }'],
  ["codeterm-prompts", "Cargo.toml", 'codeterm-service-core = "0.0.0"'],
  ["codeterm-prompts", "src/lib.rs", 'use tauri::Runtime;'],
  ["codeterm-prompts", "build.rs", 'extern crate tauri;'],
  ["codeterm-prompts", "tests/nested/leak.rs", 'use codeterm_service_core::service::ServiceContext;'],
  ["codeterm-events", "src/lib.rs", 'use codeterm_service_core::ServiceContext;'],
  ["codeterm-crash", "src/nested/leak.rs", 'fn leak(_: &ServiceContext) {}'],
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
    ["codeterm-crash", "codeterm-keepawake"],
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

test("server composition accepts core services and rejects GUI framework edges", (t) => {
  const { write, run } = fixture(t);
  write("domios-server", "Cargo.toml", '[dependencies]\ncodeterm-service-core = "0"\ncodeterm-api = "0"');
  assert.equal(run().status, 0);
  write("domios-server", "Cargo.toml", '[dependencies]\nalias = { package = "tauri", version = "2" }');
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /service core depends on tauri/);
});

test("allows the client shell to own Tauri adapters", (t) => {
  const { write, run } = fixture(t);
  write("codeterm-client-shell", "Cargo.toml", '[package]\nname = "codeterm-client-shell"\n[dependencies]\ntauri = "2"\ncodeterm-update = { path = "../codeterm-update" }\n');
  write("codeterm-client-shell", "src/lib.rs", 'pub type Runtime = tauri::Wry;');
  write("codeterm-update", "Cargo.toml", '[package]\nname = "codeterm-update"\n');
  const result = run();
  assert.equal(result.status, 0, result.stderr);
});

function client(t, dependencies) {
  const harness = fixture(t);
  harness.writeRoot(`[package]\nname = "codeterm"\n\n[dependencies]\n${dependencies}\n\n[dev-dependencies]\ncodeterm-service-core = { path = "crates/codeterm-service-core" }\n`);
  harness.write("codeterm-service-core", "Cargo.toml", '[package]\nname = "codeterm-service-core"\n');
  return harness;
}

test("process layers accept a client that links only client and shared crates", (t) => {
  const { write, run } = client(t, 'codeterm-client-shell = { path = "crates/codeterm-client-shell" }');
  write("codeterm-client-shell", "Cargo.toml", '[package]\nname = "codeterm-client-shell"\n[dependencies]\ncodeterm-update = { path = "../codeterm-update" }\n');
  write("codeterm-update", "Cargo.toml", '[package]\nname = "codeterm-update"\n[dependencies]\ncodeterm-types = { path = "../codeterm-types" }\n[dev-dependencies]\ncodeterm-pty = { path = "../codeterm-pty" }\n');
  write("codeterm-pty", "Cargo.toml", '[package]\nname = "codeterm-pty"\n');
  write("codeterm-types", "Cargo.toml", '[package]\nname = "codeterm-types"\n');
  const result = run();
  assert.equal(result.status, 0, result.stderr);
});

for (const declaration of [
  'codeterm-service-core = { path = "crates/codeterm-service-core" }',
  'renamed = { package = "domios-server", path = "crates/domios-server" }',
]) {
  test(`process layers reject a client linking the server: ${declaration}`, (t) => {
    const { write, run } = client(t, declaration);
    write("domios-server", "Cargo.toml", '[package]\nname = "domios-server"\n');
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /client crate codeterm links server crate (?:codeterm-service-core|domios-server)/);
  });
}

test("process layers reject a server crate reached through a shared crate", (t) => {
  const { write, run } = client(t, 'codeterm-update = { path = "crates/codeterm-update" }');
  write("codeterm-update", "Cargo.toml", '[package]\nname = "codeterm-update"\n[target.\'cfg(windows)\'.dependencies]\ncodeterm-pty = { path = "../codeterm-pty" }\n');
  write("codeterm-pty", "Cargo.toml", '[package]\nname = "codeterm-pty"\n');
  const result = run();
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /shared crate codeterm-update links server crate codeterm-pty/);
  assert.match(result.stderr, /codeterm -> codeterm-update -> codeterm-pty/);
});

for (const name of ["codeterm-service-core", "codeterm-update", "domios-server"]) {
  test(`process layers reject a client dependency in ${name}`, (t) => {
    const { write, run } = fixture(t);
    write(name, "Cargo.toml", `[package]\nname = "${name}"\n[dev-dependencies]\ncodeterm-client-shell = { path = "../codeterm-client-shell" }\n`);
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, new RegExp(`crate ${name} depends on client crate codeterm-client-shell`));
  });
}

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

for (const name of ["codeterm-notifications", "codeterm-events"]) {
  test(`rejects a context reference under ${name}`, (t) => {
    const { write, run } = fixture(t);
    write(name, "Cargo.toml", `[package]\nname = "${name}"`);
    write(name, "src/adapter.rs", "pub fn adapt(context: &ServiceContext) {}");
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /leaf crate names ServiceContext/);
  });
}

test("notification tier accepts models and rejects upward dependencies", (t) => {
  const { write, run } = fixture(t);
  write("codeterm-notifications", "Cargo.toml", '[dependencies]\ncodeterm-recall = "0"');
  assert.equal(run().status, 0);
  write("codeterm-recall", "Cargo.toml", '[dependencies]\ncodeterm-notifications = "0"');
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /leaf tier inversion/);
});
