import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { diffInventories, evalCfg, facadeMatch, lexRust, loadInventory, parseCargoToml, parseRenames, scanWorkspace } from "../rust-test-inventory.mjs";

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

test("evalCfg decides OS gates and leaves features and debug_assertions undecided", () => {
  const table = [
    ['target_os="windows"', "linux", false],
    ["windows", "windows", true],
    ["unix", "macos", true],
    ["not(windows)", "linux", true],
    ['all(test, target_os="macos")', "linux", false],
    ['any(windows, target_os="macos")', "linux", false],
    ["any(windows, test)", "linux", true],
    ['all(feature="embeddings", not(target_os="macos"))', "macos", false],
    ['feature="embeddings"', "linux", null],
    ["debug_assertions", "windows", null],
    ["any(windows, test) && not(windows)", "linux", true],
    ['target_family="unix"', "linux", true],
  ];
  for (const [expr, os, want] of table) assert.equal(evalCfg(expr, os), want, `${expr} on ${os}`);
});

const gatedWorkspace = () => {
  const root = tmp();
  const write = (rel, text) => {
    mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    writeFileSync(path.join(root, rel), text);
  };
  write("src-tauri/Cargo.toml", '[package]\nname = "gated-app"\n');
  write(
    "src-tauri/src/lib.rs",
    [
      "#[cfg(test)]",
      "mod tests {",
      "    #[test] fn portable() {}",
      "    #[cfg(windows)] #[test] fn windows_only() {}",
      '    #[cfg(target_os = "macos")] #[test] fn mac_only() {}',
      "    #[cfg(unix)] #[test] fn unix_only() {}",
      '    #[cfg(feature = "extra")] #[test] fn featured() {}',
      "}",
    ].join("\n"),
  );
  return root;
};
const linuxNextest = (cases) => ({ "rust-suites": { "gated-app": { "package-name": "gated-app", testcases: Object.fromEntries(cases.map((c) => [`tests::${c}`, {}])) } } });

test("diff --target-os drops foreign-gated static keys so a clean single-OS nextest list loses nothing", () => {
  const scan = scanWorkspace(gatedWorkspace());
  const nextest = linuxNextest(["portable", "unix_only", "featured"]);
  assert.deepEqual(keys(diffInventories(loadInventory(scan), loadInventory(nextest)).lost), ["tests::mac_only", "tests::windows_only"]);
  const d = diffInventories(loadInventory(scan), loadInventory(nextest), { targetOs: "linux" });
  assert.equal(d.ok, true);
  assert.deepEqual(keys(d.targetDropped), ["tests::mac_only", "tests::windows_only"]);
  const dir = tmp();
  const before = writeJson(dir, "static.json", scan);
  const run = spawnSync(process.execPath, [script, "diff", "--target-os", "linux", before, writeJson(dir, "nextest.json", nextest)], { encoding: "utf8" });
  assert.equal(run.status, 0, run.stdout + run.stderr);
  assert.match(run.stdout, /lost: 0\b/);
  assert.match(run.stdout, /dropped as foreign-gated: 2\b/);
  const bad = spawnSync(process.execPath, [script, "diff", "--target-os", "solaris", before, before], { encoding: "utf8" });
  assert.equal(bad.status, 2);
});

test("diff --target-os still fails on a portable or undecided key the nextest list lost", () => {
  const scan = loadInventory(scanWorkspace(gatedWorkspace()));
  const d = diffInventories(scan, loadInventory(linuxNextest(["unix_only"])), { targetOs: "linux" });
  assert.equal(d.ok, false);
  assert.deepEqual(keys(d.lost), ["tests::featured", "tests::portable"]);
});

test("diff --target-os on a snapshot without per-test cfg uses the cfg-gated module allowance", () => {
  const scan = scanWorkspace(gatedWorkspace());
  const snapshot = loadInventory({ tests: scan.tests.map(({ cfg, ...t }) => t), warnings: scan.warnings });
  const clean = diffInventories(snapshot, loadInventory(linuxNextest(["portable", "unix_only", "featured"])), { targetOs: "linux" });
  assert.equal(clean.ok, true);
  assert.equal(clean.targetDropped.reduce((n, x) => n + x.count, 0), 2);
  const real = diffInventories(snapshot, loadInventory(linuxNextest(["unix_only", "featured"])), { targetOs: "linux" });
  assert.equal(real.ok, false);
  assert.equal(real.lost.reduce((n, l) => n + l.count, 0), 1);
});

const row = (crate, key) => ({ crate, ...(key.includes("::") ? { module_path: key.slice(0, key.lastIndexOf("::")), test_name: key.slice(key.lastIndexOf("::") + 2) } : { module_path: "", test_name: key }) });

test("a move across a facade passes even when the crate is named unlike the module it came from", () => {
  const before = loadInventory([row("codeterm-service-core", "storage::probe::tests::slow_case"), row("codeterm-service-core", "storage::probe::tests::fast_case")]);
  const after = loadInventory([row("codeterm-persist-engine", "probe::tests::slow_case"), row("codeterm-persist-engine", "probe::tests::fast_case")]);
  const d = diffInventories(before, after);
  assert.equal(d.ok, true);
  assert.deepEqual(d.moved.map((m) => `${m.key} as ${m.as}`), ["storage::probe::tests::fast_case as probe::tests::fast_case", "storage::probe::tests::slow_case as probe::tests::slow_case"]);
  assert.equal(facadeMatch("tests::x", ["alpha::tests::x", "beta::tests::x"]), null);
  assert.equal(facadeMatch("alpha::tests::x", ["tests::x"]), "tests::x");
});

const pty = loadInventory([row("codeterm-service-core", "service::pty_io::tests::strips_osc"), row("codeterm-service-core", "service::pty_io::tests::keeps_text")]);
const ptyAfter = loadInventory([row("codeterm-pty", "pty::ansi::tests::strips_osc"), row("codeterm-service-core", "service::pty_io::tests::keeps_text")]);

test("--renames: a reviewed map line turns a lost key into renamed (reviewed); an unlisted loss still fails", () => {
  assert.equal(diffInventories(pty, ptyAfter).ok, false);
  const d = diffInventories(pty, ptyAfter, { renames: parseRenames("# P5 sanitizer move\nservice::pty_io::tests::strips_osc -> pty::ansi::tests::strips_osc\n") });
  assert.equal(d.ok, true);
  assert.deepEqual(d.renamed.map((r) => `${r.key} -> ${r.to}`), ["service::pty_io::tests::strips_osc -> pty::ansi::tests::strips_osc"]);
  assert.deepEqual(d.added, []);
  const lostAlso = loadInventory([row("codeterm-pty", "pty::ansi::tests::strips_osc")]);
  const partial = diffInventories(pty, lostAlso, { renames: parseRenames("service::pty_io::tests::strips_osc -> pty::ansi::tests::strips_osc") });
  assert.equal(partial.ok, false);
  assert.deepEqual(keys(partial.lost), ["service::pty_io::tests::keeps_text"]);
});

test("--renames: a stale map line fails, whether its old key is not lost or its new key is absent", () => {
  const notLost = diffInventories(pty, ptyAfter, { renames: parseRenames("service::pty_io::tests::strips_osc -> pty::ansi::tests::strips_osc\nservice::pty_io::tests::keeps_text -> pty::ansi::tests::keeps_text") });
  assert.equal(notLost.ok, false);
  assert.match(notLost.renameErrors.join("\n"), /keeps_text \(line 2\): service::pty_io::tests::keeps_text is not lost/);
  const absent = diffInventories(pty, ptyAfter, { renames: parseRenames("service::pty_io::tests::strips_osc -> pty::ansi::tests::strip_osc") });
  assert.equal(absent.ok, false);
  assert.match(absent.renameErrors[0], /pty::ansi::tests::strip_osc is not in the after inventory/);
  assert.throws(() => parseRenames("service::a => b"), /not "old_key -> new_key"/);
});

function gateRepo(t) {
  const repo = mkdtempSync(path.join(tmpdir(), "rti-gate-repo-"));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const git = (...args) => {
    const r = spawnSync("git", ["-C", repo, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.autocrlf=false", ...args], { encoding: "utf8" });
    assert.equal(r.status, 0, r.stderr);
    return r.stdout.trim();
  };
  const commit = (fixture, message) => {
    for (const e of readdirSync(repo)) if (e !== ".git") rmSync(path.join(repo, e), { recursive: true, force: true });
    cpSync(path.join(fixtures, fixture), repo, { recursive: true });
    git("add", "-A");
    git("commit", "-qm", message);
    return git("rev-parse", "HEAD");
  };
  git("init", "-q");
  return { repo, commit, gate: (base, ...extra) => spawnSync(process.execPath, [script, "gate", "--base", base, "--root", repo, ...extra], { encoding: "utf8" }) };
}

test("gate scans the merge-base and HEAD from git objects: a crate move exits 0, a rename exits 1 unless reviewed", (t) => {
  const g = gateRepo(t);
  const base = g.commit("move-before", "before");
  g.commit("move-after", "crate move");
  const moved = g.gate(base);
  assert.equal(moved.status, 0, moved.stdout + moved.stderr);
  assert.match(moved.stdout, /lost: 0\b/);
  assert.match(moved.stdout, /moved: 2\b/);
  g.commit("rename-after", "rename");
  const renamed = g.gate(base);
  assert.equal(renamed.status, 1, renamed.stdout + renamed.stderr);
  assert.match(renamed.stdout, /parser::tests::parses_header/);
  const map = path.join(g.repo, "..", `${path.basename(g.repo)}-renames.txt`);
  writeFileSync(map, "parser::tests::parses_header -> parser::tests::parses_head\n");
  t.after(() => rmSync(map, { force: true }));
  const reviewed = g.gate(base, "--renames", map);
  assert.equal(reviewed.status, 0, reviewed.stdout + reviewed.stderr);
  assert.match(reviewed.stdout, /renamed \(reviewed\): 1\b/);
  assert.equal(readdirSync(tmpdir()).filter((e) => e.startsWith("rti-gate-") && !e.startsWith("rti-gate-repo-")).length, 0);
});
