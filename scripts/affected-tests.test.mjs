// Caller: skill testing
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const mapper = path.join(repoRoot, "scripts", "affected-tests.mjs");

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", windowsHide: true });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout.trim();
}

async function makeDiffFixture(t, changedPath) {
  const directory = await realpath(await mkdtemp(path.join(os.tmpdir(), "codeterm-affected-tests-")));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await mkdir(path.join(directory, "src-tauri", "src"), { recursive: true });
  await mkdir(path.join(directory, "src-tauri", "crates", "codeterm-plugin", "src"), { recursive: true });
  await mkdir(path.join(directory, "src-tauri", "prompts"), { recursive: true });
  await mkdir(path.join(directory, "cli", "src"), { recursive: true });
  await mkdir(path.join(directory, "plugins", "provider-codex", "src"), { recursive: true });
  await mkdir(path.join(directory, "scripts"), { recursive: true });
  await mkdir(path.join(directory, "src"), { recursive: true });
  await mkdir(path.join(directory, "packages"), { recursive: true });

  await writeFile(path.join(directory, "src-tauri", "Cargo.toml"), `[workspace]
members = [".", "crates/codeterm-plugin", "crates/codeterm-frontend-bundle"]
resolver = "2"

[package]
name = "codeterm"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
`);
  await writeFile(path.join(directory, "src-tauri", "src", "lib.rs"), "pub mod service;\n");
  await writeFile(path.join(directory, "src-tauri", "src", "service.rs"), "pub fn current() {}\n");
  await writeFile(path.join(directory, "src-tauri", "crates", "codeterm-plugin", "Cargo.toml"), `[package]
name = "codeterm-plugin"
version = "0.1.0"
edition = "2021"
`);
  await writeFile(path.join(directory, "src-tauri", "crates", "codeterm-plugin", "src", "lib.rs"), "pub fn plugin() {}\n");
  await mkdir(path.join(directory, "src-tauri", "crates", "codeterm-frontend-bundle", "src"), { recursive: true });
  await writeFile(path.join(directory, "src-tauri", "crates", "codeterm-frontend-bundle", "Cargo.toml"), `[package]
name = "codeterm-frontend-bundle"
version = "0.1.0"
edition = "2021"
`);
  await writeFile(path.join(directory, "src-tauri", "crates", "codeterm-frontend-bundle", "src", "lib.rs"), "pub fn bundle() {}\n");
  await writeFile(path.join(directory, "cli", "Cargo.toml"), `[package]
name = "codeterm-cli"
version = "0.1.0"
edition = "2021"
`);
  await writeFile(path.join(directory, "cli", "src", "main.rs"), "fn main() {}\n");
  await writeFile(path.join(directory, "scripts", "vitest-owned-tests.json"), "[]\n");

  run("git", ["init", "--quiet"], directory);
  run("git", ["config", "user.name", "Affected tests fixture"], directory);
  run("git", ["config", "user.email", "affected-tests@example.invalid"], directory);
  run("git", ["add", "."], directory);
  run("git", ["commit", "--quiet", "-m", "fixture base"], directory);
  const base = run("git", ["rev-parse", "HEAD"], directory);

  const changedFile = path.join(directory, changedPath);
  await mkdir(path.dirname(changedFile), { recursive: true });
  await writeFile(changedFile, "export const fixtureChange = true;\n");
  run("git", ["add", "."], directory);
  run("git", ["commit", "--quiet", "-m", "fixture change"], directory);
  return { directory, base };
}

function mapFixture(directory, base) {
  return run(process.execPath, [
    mapper,
    "--repo",
    directory,
    "--base",
    base,
    "--rust-mode",
    "module-filters",
  ], directory);
}

test("maps a Rust service diff to its module filter", async (t) => {
  const fixture = await makeDiffFixture(t, "src-tauri/src/service/handler.rs");
  const output = mapFixture(fixture.directory, fixture.base);
  assert.match(output, /\.\/build\.sh nextest app 'test\(\/\^service::handler::\/\)'/);
  assert.match(output, /Cost: 1 codeterm compile \+ 1 filter/);
});

test("maps a provider plugin diff to plugin and native parity coverage", async (t) => {
  const fixture = await makeDiffFixture(t, "plugins/provider-codex/src/provider.ts");
  const output = mapFixture(fixture.directory, fixture.base);
  assert.match(output, /bash scripts\/plugin-tests\.sh provider-codex/);
  assert.match(output, /\.\/build\.sh nextest plugin 'test\(\/parity_test\/\)'/);
});

test("maps a prompt diff to the prompt contract suite", async (t) => {
  const fixture = await makeDiffFixture(t, "src-tauri/prompts/launch.md");
  const output = mapFixture(fixture.directory, fixture.base);
  assert.match(output, /\.\/build\.sh nextest app 'test\(\/\^prompts::tests::\/\)'/);
});

test("maps bundle crate changes to its supported scope and reverse dependents", async (t) => {
  const fixture = await makeDiffFixture(t, "src-tauri/crates/codeterm-frontend-bundle/src/activation.rs");
  const output = mapFixture(fixture.directory, fixture.base);
  assert.match(output, /\.\/build\.sh nextest frontend-bundle 'rdeps\(codeterm-frontend-bundle\)'/);
  assert.match(output, /Cost: 1 codeterm-frontend-bundle compile/);
});
