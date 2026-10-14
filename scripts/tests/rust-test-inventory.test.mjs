import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { diffInventories, lexRust, loadInventory, parseCargoToml, scanWorkspace } from "../rust-test-inventory.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixtures = path.join(here, "fixtures", "rust-test-inventory");
const script = path.join(here, "..", "rust-test-inventory.mjs");
const rows = (inv) => inv.tests.map((t) => `${t.crate}|${t.module_path}|${t.test_name}`);
const kinds = (inv) => inv.warnings.map((w) => `${w.kind}:${w.file}`).sort();

test("lexer keeps braces inside strings, chars, raw strings and comments out of the token stream", () => {
  const src = [
    "fn f<'a>(x: &'a str) {",
    "  let _ = '{'; let _ = b'}'; let _ = '\\''; let _ = '\\u{7d}';",
    '  let _ = "}\\" {"; let _ = r#"} " {"#; let _ = br##"}"# {"##; let _ = c"}";',
    "  /* { /* } */ { */ // }",
    "}",
  ].join("\n");
  const braces = lexRust(src).filter((t) => t.k === "punct" && (t.v === "{" || t.v === "}")).map((t) => `${t.v}@${t.line}`);
  assert.deepEqual(braces, ["{@1", "}@5"]);
});

test("lexer keeps raw identifiers distinct from keywords", () => {
  const toks = lexRust("fn r#mod() {} mod r#fn;");
  assert.deepEqual(toks.filter((t) => t.k === "ident").map((t) => `${t.v}${t.raw ? "!" : ""}`), ["fn", "r#mod!", "mod", "r#fn!"]);
});

test("cargo manifest parser reads members, package name and targets", () => {
  const toml = readFileSync(path.join(fixtures, "scan", "src-tauri", "Cargo.toml"), "utf8");
  const m = parseCargoToml(toml);
  assert.equal(m.package.name, "fixture-app");
  assert.deepEqual(m.workspace.members, [".", "crates/fixture-core"]);
  assert.deepEqual(m.tests.map((t) => t.path), ["tests/main.rs"]);
  assert.equal(m.lib.name, "fixture_app_lib");
});

test("scan resolves file layout, inline mods, mod tests; tests.rs, #[path], include! and tokio forms", () => {
  const inv = scanWorkspace(path.join(fixtures, "scan"));
  assert.deepEqual(rows(inv), [
    "fixture-app||integration_root",
    "fixture-app||smoke_runs",
    "fixture-app|helpers|helper_test",
    "fixture-app|mac_only|mac_test",
    "fixture-app|outer::blk::leaf|leaf_test",
    "fixture-app|outer::inner::tests|inner_file_test",
    "fixture-app|relocated|located_test",
    "fixture-app|relocated::sibling|sibling_test",
    "fixture-app|sentinel::tests|sentinel_probe_bounded",
    "fixture-app|tests|included_case",
    "fixture-app|tests|r#match",
    "fixture-app|tests|root_inline",
    "fixture-app|tests|tokio_flavored",
    "fixture-app|tests|tokio_plain",
    "fixture-app|tests::nested::deeper|deep_test",
    "fixture-cli|commands::tests|cli_command_test",
    "fixture-core||tool_test",
    "fixture-core|tests|core_test",
  ]);
});

test("scan reports macro bodies, inner items, unreachable files and cfg gates as warnings", () => {
  const inv = scanWorkspace(path.join(fixtures, "scan"));
  assert.deepEqual(kinds(inv), [
    "cfg-gated:src-tauri/src/mac_only.rs",
    "macro-body:src-tauri/src/generated.rs",
    "nested-item:src-tauri/src/lib.rs",
    "unreachable-file:src-tauri/src/orphan.rs",
  ]);
  assert.ok(!rows(inv).some((r) => /orphan_test|inner_item|generated_case/.test(r)));
});

test("scan CLI writes {tests, warnings} JSON and exits 0", () => {
  const out = path.join(mkdtempSync(path.join(tmpdir(), "rti-")), "inv.json");
  const run = spawnSync(process.execPath, [script, "--root", path.join(fixtures, "scan"), "--out", out], { encoding: "utf8" });
  assert.equal(run.status, 0, run.stderr);
  const inv = JSON.parse(readFileSync(out, "utf8"));
  assert.equal(inv.tests.length, 18);
  assert.equal(inv.warnings.length, 4);
});

const tmp = () => mkdtempSync(path.join(tmpdir(), "rti-"));
const staticOf = (name) => loadInventory(scanWorkspace(path.join(fixtures, name)));
const nextestJson = () => JSON.parse(readFileSync(path.join(fixtures, "nextest-move-after.json"), "utf8"));
const keys = (xs) => xs.map((x) => x.key);
const writeJson = (dir, name, value) => {
  const file = path.join(dir, name);
  writeFileSync(file, JSON.stringify(value));
  return file;
};
const runDiff = (before, after) => spawnSync(process.execPath, [script, "diff", before, after], { encoding: "utf8" });

test("diff allows a module that moves between two crates and reports it as moved", () => {
  const d = diffInventories(staticOf("move-before"), staticOf("move-after"));
  assert.equal(d.ok, true);
  assert.deepEqual(keys(d.lost), []);
  assert.deepEqual(d.moved.map((m) => `${m.key} ${m.from.join(",")}->${m.to.join(",")}`), [
    "parser::tests::parses_body fixture-app->fixture-core",
    "parser::tests::parses_header fixture-app->fixture-core",
  ]);
  const dir = tmp();
  const run = runDiff(writeJson(dir, "b.json", scanWorkspace(path.join(fixtures, "move-before"))), writeJson(dir, "a.json", scanWorkspace(path.join(fixtures, "move-after"))));
  assert.equal(run.status, 0, run.stdout + run.stderr);
  assert.match(run.stdout, /lost: 0\b/);
  assert.match(run.stdout, /moved: 2\b/);
});

test("diff fails on a renamed test and names both the lost and the added key", () => {
  const d = diffInventories(staticOf("move-before"), staticOf("rename-after"));
  assert.equal(d.ok, false);
  assert.deepEqual(keys(d.lost), ["parser::tests::parses_header"]);
  assert.deepEqual(keys(d.added), ["parser::tests::parses_head"]);
  const dir = tmp();
  const run = runDiff(writeJson(dir, "b.json", scanWorkspace(path.join(fixtures, "move-before"))), writeJson(dir, "a.json", scanWorkspace(path.join(fixtures, "rename-after"))));
  assert.equal(run.status, 1, run.stdout + run.stderr);
  assert.match(run.stdout, /parser::tests::parses_header/);
});

test("diff counts a key compiled into several binaries as a multiset", () => {
  const t = { crate: "c", module_path: "support", test_name: "shared" };
  const d = diffInventories(loadInventory([t, t]), loadInventory([t]));
  assert.equal(d.ok, false);
  assert.deepEqual(d.lost.map((l) => `${l.key}x${l.count}`), ["support::sharedx1"]);
});

test("nextest list JSON on the after side: moves pass and nextest-only keys are reported, not failed", () => {
  const after = loadInventory(nextestJson());
  assert.equal(after.format, "nextest");
  const d = diffInventories(staticOf("move-before"), after);
  assert.equal(d.ok, true);
  assert.deepEqual(keys(d.moved), ["parser::tests::parses_body", "parser::tests::parses_header"]);
  assert.deepEqual(keys(d.nextestOnly), ["tests::generated_case"]);
  assert.deepEqual(keys(d.added), []);
});

test("a static key missing from nextest fails", () => {
  const json = nextestJson();
  delete json["rust-suites"]["fixture-core"].testcases["parser::tests::parses_body"];
  const d = diffInventories(staticOf("move-after"), loadInventory(json));
  assert.equal(d.ok, false);
  assert.deepEqual(keys(d.lost), ["parser::tests::parses_body"]);
  const dir = tmp();
  const run = runDiff(writeJson(dir, "static.json", scanWorkspace(path.join(fixtures, "move-after"))), writeJson(dir, "nextest.json", json));
  assert.equal(run.status, 1, run.stdout + run.stderr);
  assert.match(run.stdout, /parser::tests::parses_body/);
});

test("nextest list JSON on the before side is diffed strictly", () => {
  assert.equal(diffInventories(loadInventory(nextestJson()), loadInventory(nextestJson())).ok, true);
  const d = diffInventories(loadInventory(nextestJson()), staticOf("move-after"));
  assert.equal(d.ok, false);
  assert.deepEqual(keys(d.lost), ["tests::generated_case"]);
});

test("loadInventory detects the format by shape and refuses anything else", () => {
  const t = { crate: "c", module_path: "", test_name: "x" };
  assert.equal(loadInventory([t]).format, "static");
  assert.equal(loadInventory({ tests: [t], warnings: [] }).format, "static");
  assert.throws(() => loadInventory({ suites: {} }), /unrecognised/);
});

test("diff CLI on an identical inventory exits 0 with zero lost", () => {
  const file = writeJson(tmp(), "inv.json", scanWorkspace(path.join(fixtures, "scan")));
  const run = runDiff(file, file);
  assert.equal(run.status, 0, run.stdout + run.stderr);
  assert.match(run.stdout, /lost: 0\b/);
});
