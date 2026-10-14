import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { lexRust, parseCargoToml, scanWorkspace } from "../rust-test-inventory.mjs";

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
