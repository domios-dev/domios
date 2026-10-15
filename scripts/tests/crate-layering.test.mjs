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
  for (const name of ["codeterm-events", "codeterm-keepawake"]) write(name, "Cargo.toml", '[package]\nname = "' + name + '"');
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
]) {
  test(`rejects ${name}/${file}: ${text}`, (t) => {
    const { write, run } = fixture(t);
    write(name, file, text);
    const result = run();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /leaf crate names/);
  });
}
for (const name of ["codeterm-events", "codeterm-keepawake"]) {
  test(`rejects framework dependency in ${name}`, (t) => {
    const { write, run } = fixture(t);
    write(name, "Cargo.toml", 'tauri = "2"');
    const result = run();
    assert.equal(result.status, 1);
    assert.match(result.stderr, /leaf crate names tauri/);
  });
}
