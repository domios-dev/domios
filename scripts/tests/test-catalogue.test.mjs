import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { globToRegExp, renderReadme, runCommands, schemaErrors, selectRunners, validateRepo } from "../test-catalogue.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.join(here, "..", "..");
const cli = path.join(here, "..", "test-catalogue.mjs");
const schema = JSON.parse(readFileSync(path.join(repo, "tests", "catalogue.schema.json"), "utf8"));

const CASE = 'test("case", () => {});\n';
const PW_TITLE = "@golden picker keeps its workspace row";
const baseCatalogue = () => ({
  version: 1,
  test_looking: {
    patterns: ["**/*.test.{ts,mjs}", "**/*.spec.ts", "**/Cargo.toml"],
    not_tests: [{ pattern: "fixtures/**", reason: "fixture data read by the tests" }],
  },
  runners: [
    runner("ts-unit", { discovery: { method: "glob", patterns: ["src/**/*.test.ts"], exclude_list_file: "scripts/vitest-owned-tests.json" } }),
    runner("ts-vitest", { discovery: { method: "list-file", list_file: "scripts/vitest-owned-tests.json" } }),
    runner("node-scripts", { kind: "contract", lang: "js", exec: ["node", "--test"], discovery: { method: "glob", patterns: ["scripts/*.test.mjs"] } }),
    runner("pw", { kind: "e2e-curated", tier: "manual", needs_instance: true, needs_live_agent: true, discovery: { method: "playwright-list", patterns: ["tests/pw/*.spec.ts"] } }),
    runner("rust-app", { lang: "rust", package: "fixture-app", manifest: "src-tauri/Cargo.toml", tier: "release", discovery: { method: "nextest-list", fallback: "static-scan" } }),
  ],
});
function runner(id, extra) {
  return { id, kind: "unit", lang: "ts", runner: `run ${id}`, tier: "pr", needs_instance: false, needs_live_agent: false, budget_secs: 60, owner_doc: "README.md", ...extra };
}
const baseOverlay = () => ({
  version: 1,
  stale: [{ runner: "pw", target: { spec: "tests/pw/picker.spec.ts", title: PW_TITLE }, reason: "drives a removed test id", source: { commit: "ddfca8ee7" } }],
  flaky: [{ runner: "rust-app", target: { binary: "fixture-app", test: "probe::tests::bounded" }, reason: "load-sensitive deadline", source: { task: "task_abc123" } }],
  platform_red: [],
});

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), "test-catalogue-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const write = (rel, text) => {
    mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    writeFileSync(path.join(root, rel), text);
  };
  const json = (rel, value) => write(rel, JSON.stringify(value, null, 2));
  mkdirSync(path.join(root, "tests"), { recursive: true });
  copyFileSync(path.join(repo, "tests", "catalogue.schema.json"), path.join(root, "tests", "catalogue.schema.json"));
  write("README.md", "owner doc\n");
  write("src/a.test.ts", CASE);
  write("src/b.test.ts", CASE);
  write("fixtures/c.test.ts", "");
  write("scripts/x.test.mjs", 'import { test } from "node:test";\ntest("passes", () => {});\n');
  write("tests/pw/picker.spec.ts", `test("${PW_TITLE}", () => {});\n`);
  write("src-tauri/Cargo.toml", '[package]\nname = "fixture-app"\n');
  write("src-tauri/src/lib.rs", "mod probe;\n");
  write("src-tauri/src/probe.rs", "#[cfg(test)]\nmod tests {\n    #[test]\n    fn bounded() {}\n}\n");
  json("scripts/vitest-owned-tests.json", ["src/b.test.ts"]);
  const state = { catalogue: baseCatalogue(), overlay: baseOverlay() };
  const save = () => {
    json("tests/catalogue.json", state.catalogue);
    json("tests/catalogue.overlay.json", state.overlay);
    write("tests/README.md", renderReadme(state.catalogue, state.overlay));
  };
  save();
  const classes = () => validateRepo(root, { env: {} }).errors.map((e) => e.cls);
  return { root, write, json, state, save, classes, errors: () => validateRepo(root, { env: {} }).errors };
}

test("a clean fixture validates with every runner's membership discovered", (t) => {
  const f = fixture(t);
  const { errors, found } = validateRepo(f.root, { env: {} });
  assert.deepEqual(errors, []);
  const files = Object.fromEntries(found.map((x) => [x.runner.id, x.tests ? x.tests.length : x.files]));
  assert.deepEqual(files["ts-unit"], ["src/a.test.ts"]);
  assert.deepEqual(files["ts-vitest"], ["src/b.test.ts"]);
  assert.deepEqual(files["rust-app"], 1);
});

test("unmatched: a test-looking file no runner discovers fails, and passes once a runner owns it", (t) => {
  const f = fixture(t);
  f.write("lib/zz_probe.test.ts", CASE);
  const red = f.errors();
  assert.deepEqual(red.map((e) => e.cls), ["unmatched"]);
  assert.match(red[0].message, /lib\/zz_probe\.test\.ts/);
  f.state.catalogue.runners[0].discovery.patterns.push("lib/**/*.test.ts");
  f.save();
  assert.deepEqual(f.classes(), []);
});

test("empty: an owned test file with nothing in it fails until it holds a test", (t) => {
  const f = fixture(t);
  f.write("src/zz_probe.test.ts", "// nothing yet\n");
  const red = f.errors();
  assert.deepEqual(red.map((e) => e.cls), ["empty"]);
  assert.match(red[0].message, /src\/zz_probe\.test\.ts/);
  f.write("src/zz_probe.test.ts", CASE);
  assert.deepEqual(f.classes(), []);
});

test("double-owned: a file two runners discover fails", (t) => {
  const f = fixture(t);
  f.state.catalogue.runners[2].discovery.patterns.push("src/a.test.ts");
  f.save();
  assert.deepEqual(f.classes(), ["double-owned"]);
  f.state.catalogue.runners[2].discovery.patterns.pop();
  f.save();
  assert.deepEqual(f.classes(), []);
});

test("invalid field: a schema violation fails before any discovery, and the fixed field passes", (t) => {
  const f = fixture(t);
  f.state.catalogue.runners[0].tier = "nightly";
  f.state.catalogue.runners[2].needs_instance = true;
  f.state.catalogue.runners[3].owner = "nobody";
  f.save();
  const red = f.errors();
  assert.ok(red.length >= 3 && red.every((e) => e.cls === "invalid-field"), JSON.stringify(red));
  assert.ok(red.some((e) => /tier: must be one of/.test(e.message)));
  assert.ok(red.some((e) => /needs_instance: must be false/.test(e.message)));
  assert.ok(red.some((e) => /unknown field owner/.test(e.message)));
  f.state.catalogue = baseCatalogue();
  f.state.catalogue.runners[1].owner_doc = "docs/missing.md";
  f.save();
  assert.deepEqual(f.classes(), ["invalid-field"]);
  f.state.catalogue = baseCatalogue();
  f.save();
  assert.deepEqual(f.classes(), []);
});

test("unresolved overlay key: a Playwright title or Rust test that is not there fails", (t) => {
  const f = fixture(t);
  f.state.overlay.stale[0].target.title = "@golden a title nobody wrote";
  f.state.overlay.flaky[0].target.test = "probe::tests::renamed";
  f.save();
  const red = f.errors();
  assert.deepEqual(red.map((e) => e.cls), ["overlay", "overlay"]);
  assert.match(red[0].message, /title is not in tests\/pw\/picker\.spec\.ts/);
  assert.match(red[1].message, /probe::tests::renamed is not in the static-scan inventory/);
  f.state.overlay = baseOverlay();
  f.state.overlay.flaky[0].runner = "ts-unit";
  f.save();
  assert.deepEqual(f.classes(), ["overlay"]);
  f.state.overlay = baseOverlay();
  f.save();
  assert.deepEqual(f.classes(), []);
});

test("vitest disagreement: a missing listed file, an overlapping runner or a runner-discovery mismatch fails", (t) => {
  const f = fixture(t);
  f.json("scripts/vitest-owned-tests.json", ["src/b.test.ts", "src/gone.test.ts"]);
  assert.deepEqual(f.classes(), ["vitest"]);
  f.json("scripts/vitest-owned-tests.json", ["src/b.test.ts"]);
  delete f.state.catalogue.runners[0].discovery.exclude_list_file;
  f.save();
  assert.deepEqual(f.classes(), ["vitest"]);
  f.state.catalogue = baseCatalogue();
  f.save();
  const disagree = validateRepo(f.root, { env: {}, tsUnitRunner: () => ["src/a.test.ts", "src/b.test.ts"] }).errors;
  assert.deepEqual(disagree.map((e) => e.cls), ["vitest"]);
  assert.match(disagree[0].message, /runner-only src\/b\.test\.ts/);
  assert.deepEqual(validateRepo(f.root, { env: {}, tsUnitRunner: () => ["src/a.test.ts"] }).errors, []);
});

test("stale README: a hand edit fails until the README is regenerated", (t) => {
  const f = fixture(t);
  f.write("tests/README.md", renderReadme(f.state.catalogue, f.state.overlay) + "hand edit\n");
  assert.deepEqual(f.classes(), ["readme"]);
  const regen = spawnSync(process.execPath, [cli, "readme", "--root", f.root], { encoding: "utf8" });
  assert.equal(regen.status, 0, regen.stderr);
  assert.deepEqual(f.classes(), []);
  const check = spawnSync(process.execPath, [cli, "readme", "--check", "--root", f.root], { encoding: "utf8" });
  assert.equal(check.status, 0, check.stderr);
});

test("validate CLI exits 1 naming an unmatched file and 0 once it is gone", (t) => {
  const f = fixture(t);
  f.write("src/zz_probe.test.mjs", "");
  const red = spawnSync(process.execPath, [cli, "validate", "--root", f.root], { encoding: "utf8" });
  assert.equal(red.status, 1);
  assert.match(red.stderr, /\[unmatched\] src\/zz_probe\.test\.mjs/);
  rmSync(path.join(f.root, "src/zz_probe.test.mjs"));
  const green = spawnSync(process.execPath, [cli, "validate", "--root", f.root], { encoding: "utf8" });
  assert.equal(green.status, 0, green.stderr);
});

test("select keeps instance and live-agent runners out of pr and reports every marker as not healthy", (t) => {
  const f = fixture(t);
  const sel = selectRunners(f.state.catalogue, f.state.overlay, { tier: "pr" });
  assert.deepEqual(sel.runners.map((r) => r.id), ["ts-unit", "ts-vitest", "node-scripts"]);
  assert.deepEqual(sel.stale.map((m) => [m.runner, m.in_selection]), [["pw", false]]);
  assert.deepEqual(sel.flaky.map((m) => [m.runner, m.in_selection]), [["rust-app", false]]);
  const json = spawnSync(process.execPath, [cli, "select", "--tier", "release", "--format", "json", "--root", f.root], { encoding: "utf8" });
  assert.equal(json.status, 0, json.stderr);
  const out = JSON.parse(json.stdout);
  assert.deepEqual(out.runners.map((r) => [r.id, r.test_count]), [["rust-app", 1]]);
  assert.equal(out.flaky[0].in_selection, true);
  assert.equal(out.stale.length, 1);
  f.state.catalogue.runners[3].status = "suspended";
  assert.deepEqual(selectRunners(f.state.catalogue, f.state.overlay, { tier: "manual" }).suspended, ["pw"]);
  const matrix = spawnSync(process.execPath, [cli, "select", "--tier", "pr", "--format", "github-matrix", "--root", f.root], { encoding: "utf8" });
  assert.deepEqual(JSON.parse(matrix.stdout).include.map((x) => x.id), ["ts-unit", "ts-vitest", "node-scripts"]);
});

test("nextest list JSON replaces the static scan for the packages it lists", (t) => {
  const f = fixture(t);
  const list = path.join(f.root, "nextest.json");
  writeFileSync(list, JSON.stringify({ "rust-suites": { "fixture-app": { "package-name": "fixture-app", testcases: { "probe::tests::bounded": {}, "probe::tests::generated": {} } } } }));
  const { errors, found } = validateRepo(f.root, { env: { CODETERM_CATALOGUE_NEXTEST_JSON: list } });
  assert.deepEqual(errors, []);
  const rust = found.find((x) => x.runner.id === "rust-app");
  assert.equal(rust.source, "nextest-list");
  assert.equal(rust.tests.length, 2);
});

test("run skips stale markers on their platform only, for node:test and unittest runners", () => {
  const overlay = {
    platform_red: [],
    stale: [
      { runner: "node", target: { spec: "a.test.mjs", title: "red (on linux)" }, platforms: ["linux"] },
      { runner: "py", target: { spec: "tests/test_a.py", title: "test_red" } },
    ],
  };
  const node = { id: "node", exec: ["node", "--test"] };
  assert.deepEqual(runCommands(node, ["a.test.mjs"], overlay, { platform: "linux", node: "NODE" }), [["NODE", "--test", "--test-skip-pattern=^red \\(on linux\\)$", "a.test.mjs"]]);
  assert.deepEqual(runCommands(node, ["a.test.mjs"], overlay, { platform: "windows", node: "NODE" }), [["NODE", "--test", "a.test.mjs"]]);
  const py = runCommands({ id: "py", exec: ["python3", "-m", "unittest"], exec_each: true }, ["tests/test_a.py", "tests/test_b.py"], overlay, { python: "PY" });
  assert.equal(py.length, 2);
  assert.deepEqual(py[0].filter((a) => !a.includes("\n")), ["PY", "-c", "tests/test_a.py", "--skip", "test_red"]);
  assert.throws(() => runCommands({ id: "x", runner: "pnpm x" }, ["a"], overlay), /has no exec/);
});

test("run executes a unittest runner, skips the stale case and fails a file with no test cases", (t) => {
  const python = ["python3", "python"].find((p) => spawnSync(p, ["-c", ""]).status === 0);
  if (!python) return t.skip("no python on PATH");
  const f = fixture(t);
  f.write("pytests/test_ok.py", "import unittest\nclass T(unittest.TestCase):\n    def test_green(self): pass\n    def test_red(self): self.fail('stale')\n");
  f.state.catalogue.test_looking.patterns.push("**/test_*.py");
  f.state.catalogue.runners.push(runner("py", { lang: "python", exec: ["python3", "-m", "unittest"], discovery: { method: "glob", patterns: ["pytests/test_*.py"] } }));
  f.state.overlay.stale.push({ runner: "py", target: { spec: "pytests/test_ok.py", title: "test_red" }, reason: "kept red on purpose", source: { task: "task_abc123" } });
  f.save();
  assert.deepEqual(f.classes(), []);
  const ok = spawnSync(process.execPath, [cli, "run", "--entry", "py", "--root", f.root], { cwd: f.root, encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stdout + ok.stderr);
  assert.match(ok.stdout, /skip \(overlay\): pytests\.test_ok\.T\.test_red/);
  f.write("pytests/test_pytest_style.py", "def test_plain():\n    assert True\n");
  const empty = spawnSync(process.execPath, [cli, "run", "--entry", "py", "--root", f.root], { cwd: f.root, encoding: "utf8" });
  assert.equal(empty.status, 1);
  assert.match(empty.stdout, /no unittest test cases in pytests\/test_pytest_style\.py/);
});

test("globs: * stays in a segment, ** crosses segments, braces alternate", () => {
  assert.ok(globToRegExp("scripts/*.test.mjs").test("scripts/a.test.mjs"));
  assert.ok(!globToRegExp("scripts/*.test.mjs").test("scripts/tests/a.test.mjs"));
  assert.ok(globToRegExp("**/*.test.{ts,tsx}").test("a.test.tsx"));
  assert.ok(globToRegExp("src/**/*.test.ts").test("src/x/y/z.test.ts"));
  assert.ok(globToRegExp("src/**/*.test.ts").test("src/z.test.ts"));
  assert.ok(!globToRegExp("src/**/*.test.ts").test("srcx/z.test.ts"));
});

test("the overlay schema refuses a marker without a source", () => {
  const overlay = baseOverlay();
  delete overlay.stale[0].source;
  assert.match(schemaErrors(schema, schema.$defs.overlayFile, overlay, "overlay").join("\n"), /missing required field source/);
});

test("platform-red markers need platforms and an owner, report as their own class and skip only on their platforms", (t) => {
  const f = fixture(t);
  const red = { runner: "node-scripts", target: { spec: "scripts/x.test.mjs", title: "passes" }, reason: "red on linux only", source: { task: "task_abc123" } };
  f.state.overlay.platform_red.push(red);
  f.save();
  const missing = f.errors();
  assert.ok(missing.length && missing.every((e) => e.cls === "invalid-field"), JSON.stringify(missing));
  assert.ok(missing.some((e) => /missing required field platforms/.test(e.message)) && missing.some((e) => /missing required field owner/.test(e.message)));
  Object.assign(red, { platforms: ["linux"], owner: "task_abc123" });
  f.save();
  assert.deepEqual(f.classes(), []);
  const sel = selectRunners(f.state.catalogue, f.state.overlay, { tier: "pr" });
  assert.deepEqual(sel.stale.length, 1);
  assert.deepEqual(sel.platform_red.map((m) => [m.runner, m.in_selection]), [["node-scripts", true]]);
  const runner = f.state.catalogue.runners[2];
  assert.deepEqual(runCommands(runner, ["scripts/x.test.mjs"], f.state.overlay, { platform: "linux", node: "NODE" })[0].slice(2, 3), ["--test-skip-pattern=^passes$"]);
  assert.deepEqual(runCommands(runner, ["scripts/x.test.mjs"], f.state.overlay, { platform: "windows", node: "NODE" }), [["NODE", "--test", "scripts/x.test.mjs"]]);
  const stale = spawnSync(process.execPath, [cli, "stale", "--root", f.root], { encoding: "utf8" });
  assert.match(stale.stdout, /PLATFORM-RED[^\n]*: 1\n  node-scripts: scripts\/x\.test\.mjs :: passes on linux/);
});

test("owner rule: a live-provider runner must be a curated or eval runner at tier manual", (t) => {
  const f = fixture(t);
  f.state.catalogue.runners[3].kind = "e2e-plain";
  f.save();
  const red = f.errors();
  assert.deepEqual(red.map((e) => e.cls), ["invalid-field"]);
  assert.match(red[0].message, /runners\[3\]\.kind: must be one of e2e-curated, eval/);
  f.state.catalogue.runners[3].kind = "e2e-curated";
  f.state.catalogue.runners[3].tier = "release";
  f.save();
  assert.match(f.errors()[0].message, /runners\[3\]\.tier: must be "manual"/);
  f.state.catalogue.runners[3].tier = "manual";
  f.save();
  assert.deepEqual(f.classes(), []);
});

test("depends_on: a file the runner imports from outside its discovery must exist", (t) => {
  const f = fixture(t);
  f.write("evals/restart/restart.mjs", "export const restartObserved = () => true;\n");
  f.state.catalogue.runners[2].depends_on = ["evals/restart/restart.mjs"];
  f.save();
  assert.deepEqual(f.classes(), []);
  rmSync(path.join(f.root, "evals/restart/restart.mjs"));
  const red = f.errors();
  assert.deepEqual(red.map((e) => e.cls), ["discovery"]);
  assert.match(red[0].message, /node-scripts: depends_on evals\/restart\/restart\.mjs does not exist/);
});

test("contains and lacks split one glob by content, so live specs and plain specs each have one owner", (t) => {
  const f = fixture(t);
  f.write("tests/pw/live.spec.ts", 'test(\n  "@live real agent replies", () => {});\n');
  f.write("tests/pw/notes.spec.ts", '// the @live variants live elsewhere\ntest("@golden plain", () => {});\n');
  assert.deepEqual(f.classes(), []);
  const live = runner("pw-live", { kind: "e2e-curated", tier: "manual", needs_instance: true, needs_live_agent: true, discovery: { method: "playwright-list", patterns: ["tests/pw/*.spec.ts"], contains: String.raw`\btest(?:\.\w+)?\(\s*"[^"]*@live` } });
  f.state.catalogue.runners.push(live);
  f.save();
  assert.deepEqual(f.classes(), ["double-owned"]);
  f.state.catalogue.runners[3].discovery.lacks = live.discovery.contains;
  f.save();
  const { errors, found } = validateRepo(f.root, { env: {} });
  assert.deepEqual(errors, []);
  assert.deepEqual(found.find((x) => x.runner.id === "pw-live").files, ["tests/pw/live.spec.ts"]);
  assert.deepEqual(found.find((x) => x.runner.id === "pw").files, ["tests/pw/notes.spec.ts", "tests/pw/picker.spec.ts"]);
});
