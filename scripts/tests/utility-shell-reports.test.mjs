import assert from "node:assert/strict";
import { once } from "node:events";
import { spawn, spawnSync } from "node:child_process";
import {
  appendFileSync,
  chmodSync,
  closeSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test, { after } from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : (existsSync("/bin/bash") ? "/bin/bash" : "bash");
const realGit = spawnSync(bash, ["-c", "command -v git"], { encoding: "utf8" }).stdout.trim();
const helperPath = path.join(root, "scripts/dev-notify.sh");
const qaReportPath = path.join(root, "scripts/qa-report.sh");
const buildPath = path.join(root, "build.sh");
const connectionKeys = [
  "CODETERM_INSTANCE",
  "CODETERM_API_PORT",
  "CODETERM_API_URL",
  "CODETERM_API_TOKEN",
  "CODETERM_SOCKET_PATH",
  "CODETERM_DATA_DIR",
  "CODETERM_TAB_ID",
  "CODETERM_SESSION_ID",
  "CODETERM_SESSION_BINDING_NONCE",
  "CODETERM_WORKER_ID",
  "CODETERM_PARENT_TAB_ID",
  "CODETERM_TEAM_ID",
];

function shellQuote(value) {
  return "'" + String(value).replace(/'/g, "'\\''") + "'";
}

const cliNames = ["codeterm", "codeterm.exe"];
const shadowPaths = new Map();
let shadowRoot;

after(() => {
  if (shadowRoot) rmSync(shadowRoot, { recursive: true, force: true });
});

// The live CLI reaches the owner's daemon even with every selector scrubbed, so it must not resolve at all.
function pathWithoutInstalledCli(searchPath = process.env.PATH ?? "") {
  return searchPath
    .split(path.delimiter)
    .filter(Boolean)
    .flatMap((entry) => {
      if (!cliNames.some((name) => existsSync(path.join(entry, name)))) return [entry];
      if (process.platform === "win32") return [];
      const source = path.resolve(entry);
      if (!shadowPaths.has(source)) {
        shadowRoot ??= mkdtempSync(path.join(os.tmpdir(), "utility-shell-path-"));
        const shadow = path.join(shadowRoot, String(shadowPaths.size));
        mkdirSync(shadow);
        for (const name of readdirSync(source)) {
          if (!cliNames.includes(name)) symlinkSync(path.join(source, name), path.join(shadow, name));
        }
        shadowPaths.set(source, shadow);
      }
      return [shadowPaths.get(source)];
    })
    .join(path.delimiter);
}

function cleanEnv(extra = {}) {
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (/^(CODETERM_|BUILD_NOTIFY_|DEV_NOTIFY_)/.test(key)) delete env[key];
  }
  for (const key of ["QA_NOTIFY_TAB", "QA_REPORT_DIR"]) {
    delete env[key];
  }
  return { ...env, PATH: pathWithoutInstalledCli(), ...extra };
}

function writeFakeCli(cliPath, recordPath) {
  mkdirSync(path.dirname(cliPath), { recursive: true });
  const source =
    "#!/usr/bin/env node\n" +
    "const fs = require('node:fs');\n" +
    "const args = process.argv.slice(2);\n" +
    "if (args[0] === 'tab' && args[1] === 'inspect') { const model = process.env.BUILD_NOTIFICATION_TAB_MODEL; process.stdout.write(JSON.stringify({result: model ? JSON.parse(fs.readFileSync(model, 'utf8')) : []})); process.exit(0); }\n" +
    "if (args[0] === 'pad' && args[1] === 'path') { process.stdout.write(process.env.BUILD_NOTIFICATION_PAD_PATH || process.cwd()); process.exit(0); }\n" +
    "const fromFile = args.indexOf('--from-file');\n" +
    "const fromPad = args.indexOf('--from-pad');\n" +
    "const bodyFile = fromFile >= 0 ? args[fromFile + 1] : (fromPad >= 0 ? require('node:path').join(process.env.BUILD_NOTIFICATION_PAD_PATH || process.cwd(), args[fromPad + 1]) : null);\n" +
    "const keys = " + JSON.stringify(connectionKeys) + ";\n" +
    "const env = {};\n" +
    "for (const key of keys) env[key] = process.env[key] ?? null;\n" +
    "const record = { sentAt: Date.now(), executable: process.argv[1], args, bodyFile, body: bodyFile ? fs.readFileSync(bodyFile, 'utf8') : null, env };\n" +
    "fs.appendFileSync(" + JSON.stringify(recordPath) + ", JSON.stringify(record) + '\\n');\n" +
    "process.exit(Number(process.env.BUILD_NOTIFICATION_SEND_EXIT || 0));\n";
  writeFileSync(cliPath, source, { mode: 0o755 });
  chmodSync(cliPath, 0o755);
}

function makeHarness() {
  const dir = mkdtempSync(path.join(os.tmpdir(), "utility-shell-reports-"));
  const parentBin = path.join(dir, "parent-bin");
  const devBin = path.join(dir, "dev-bin");
  const parentRecord = path.join(dir, "parent-record.jsonl");
  const devRecord = path.join(dir, "dev-record.jsonl");
  const parentCli = path.join(parentBin, "codeterm");
  const devCli = path.join(devBin, "codeterm");
  const padPath = path.join(dir, "agent-pad");
  const fixtureBuild = path.join(dir, "build.sh");
  const buildSource = readFileSync(buildPath, "utf8").replaceAll("\r\n", "\n");
  const gateway = buildSource.match(/^enter_cargo_gateway\(\) \{[^]*?^\}/m)?.[0];
  assert.ok(gateway, "build fixture must stub the Cargo gateway");
  const authority = "source scripts/lane-rust-guard.sh";
  const setup = "*) setup_env ;;";
  assert.ok(buildSource.includes(authority) && buildSource.includes(setup), "build fixture must isolate host authorization and toolchain setup");
  writeFileSync(fixtureBuild, buildSource.replace(gateway, 'enter_cargo_gateway() { :; }')
    .replace(authority, 'require_canonical_rust_tests() { :; }')
    .replace(setup, '*) : ;;'));
  writeFakeCli(parentCli, parentRecord);
  writeFakeCli(devCli, devRecord);
  mkdirSync(padPath, { recursive: true });
  return {
    dir,
    parentBin,
    devBin,
    parentRecord,
    devRecord,
    parentCli,
    devCli,
    padPath,
    env: cleanEnv({ BUILD_NOTIFICATION_FIXTURE_PATH: fixtureBuild, BUILD_NOTIFICATION_PAD_PATH: padPath, PATH: parentBin + path.delimiter + pathWithoutInstalledCli() }),
  };
}

test("harness_excludes_installed_cli_and_preserves_system_utilities", () => {
  const env = cleanEnv();
  for (const name of cliNames) {
    const result = runBash(["-c", `command -v ${name}`], env);
    assert.equal(result.status, 1, result.stdout + result.stderr);
    assert.equal(result.stdout, "");
  }
  const result = runBash(["-c", "command -v dirname && command -v mkdir"], env);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(result.stdout.trim().split("\n").length, 2);
});

test("harness_preserves_utilities_beside_installed_cli", { skip: process.platform === "win32" }, () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "utility-shell-shared-bin-"));
  try {
    for (const name of cliNames) writeFileSync(path.join(dir, name), "#!/bin/sh\nexit 99\n", { mode: 0o755 });
    for (const name of ["dirname", "mkdir"]) {
      const result = runBash(["-c", `command -v ${name}`], cleanEnv());
      assert.equal(result.status, 0, result.stderr);
      symlinkSync(result.stdout.trim(), path.join(dir, name));
    }
    const env = cleanEnv({ PATH: pathWithoutInstalledCli(dir + path.delimiter + dir) });
    for (const name of cliNames) {
      const result = runBash(["-c", `command -v ${name}`], env);
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.equal(result.stdout, "");
    }
    const created = path.join(dir, "created");
    const result = runBash(["-c", 'command -v dirname && command -v mkdir && mkdir "$1" && dirname "$1"', "_", created], env);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.ok(existsSync(created));
    assert.equal(result.stdout.trim().split("\n").at(-1), dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("harness_resolves_only_the_fake_codeterm", () => {
  const h = makeHarness();
  try {
    assert.deepEqual(Object.keys(h.env).filter((key) => /^(CODETERM_|BUILD_NOTIFY_|DEV_NOTIFY_)/.test(key)), []);
    const result = runBash(["-c", "type -P -a codeterm codeterm.exe"], h.env);
    const resolved = result.stdout.split("\n").filter(Boolean);
    assert.equal(resolved.length, 1, result.stdout);
    assert.equal(spawnSync(bash, ["-c", 'cygpath -m "$1" 2>/dev/null || printf %s "$1"', "_", resolved[0]], { encoding: "utf8" }).stdout.trim().toLowerCase(),
      h.parentCli.replaceAll("\\", "/").toLowerCase());
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

function records(recordPath) {
  if (!existsSync(recordPath)) return [];
  return readFileSync(recordPath, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function runBash(args, env, cwd = root) {
  const commandArgs = args.map(arg => arg === buildPath && env.BUILD_NOTIFICATION_FIXTURE_PATH ? env.BUILD_NOTIFICATION_FIXTURE_PATH : arg);
  const runEnv = commandArgs[0] === env.BUILD_NOTIFICATION_FIXTURE_PATH
    ? { CODETERM_DEV_INSTANCE: "reportfixture", CODETERM_DEV_API_PORT: "17695", ...env }
    : env;
  return spawnSync(bash, commandArgs, { cwd, env: runEnv, encoding: "utf8" });
}

function runQa(harness, label, commands, extraEnv = {}) {
  return runBash(
    [qaReportPath, label, ...commands],
    {
      ...harness.env,
      QA_REPORT_DIR: path.join(harness.dir, "reports"),
      ...extraEnv,
    },
  );
}

test("qa_report_pass_sends_one_summary", () => {
  const h = makeHarness();
  try {
    const cargoLine = "printf '%s\\n' 'test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'";
    const result = runQa(h, "smoke", ["true", cargoLine], {
      QA_NOTIFY_TAB: "agent-tab",
      CODETERM_INSTANCE: "parent",
      CODETERM_API_PORT: "7685",
      CODETERM_API_URL: "http://127.0.0.1:7685",
      CODETERM_API_TOKEN: "parent-token",
      CODETERM_SOCKET_PATH: "parent.sock",
      CODETERM_DATA_DIR: path.join(h.dir, "parent-data"),
      CODETERM_TAB_ID: "parent-caller",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /QA smoke: PASS \(2\/2 ok\)/);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args, ["send", "--tab", "agent-tab", "--from-file", sent[0].bodyFile]);
    assert.match(sent[0].body, /test result: ok\. 3 passed; 0 failed/);
    assert.match(sent[0].body, /QA smoke: PASS \(2\/2 ok\)/);
    assert.equal(sent[0].env.CODETERM_INSTANCE, null);
    assert.equal(sent[0].env.CODETERM_API_PORT, null);
    assert.equal(sent[0].env.CODETERM_API_URL, "http://127.0.0.1:7685");
    assert.equal(sent[0].env.CODETERM_API_TOKEN, "parent-token");
    assert.equal(sent[0].env.CODETERM_SOCKET_PATH, "parent.sock");
    assert.equal(sent[0].env.CODETERM_DATA_DIR, path.join(h.dir, "parent-data"));
    assert.equal(sent[0].env.CODETERM_TAB_ID, "parent-caller");
    assert.equal(records(h.devRecord).length, 0);
    const reportRoot = path.join(h.dir, "reports");
    const runDir = path.join(reportRoot, readdirSync(reportRoot)[0]);
    assert.ok(existsSync(path.join(runDir, "full.log")));
    assert.ok(existsSync(path.join(runDir, "command-1.log")));
    assert.ok(existsSync(path.join(runDir, "command-2.log")));
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("qa_report_pipefail_catches_false_pipe_tail", () => {
  const h = makeHarness();
  try {
    const result = runQa(h, "pipefail", ["false | tail -1"], { QA_NOTIFY_TAB: "agent-tab" });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /\[1\/1\] rc=1 false \| tail -1/);
    assert.match(result.stdout, /QA pipefail: FAIL \(0\/1 ok\)/);
    assert.equal(records(h.parentRecord).length, 2);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("qa_report_zero_tests_flag", () => {
  const h = makeHarness();
  try {
    const cargo = "printf '%s\\n' 'running 0 tests' 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'";
    const vitest = "printf '%s\\n' 'No test files found'";
    const node = "printf '%s\\n' '# tests 0' '# pass 0' '# fail 0'";
    const result = runQa(h, "empty", [cargo, vitest, node]);
    assert.equal(result.status, 1);
    assert.equal((result.stdout.match(/ZERO TESTS RAN/g) ?? []).length, 3);
    assert.match(result.stdout, /QA empty: FAIL \(0\/3 ok\)/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("qa_report_counts_executed_cargo_targets_without_promoting_empty_runs", () => {
  const h = makeHarness();
  const empty = "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out";
  const one = "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
  const cases = [
    ["mixed", one + "\n" + empty, 0],
    ["empty_first", empty + "\n" + one, 0],
    ["ten", "test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out", 0],
    ["empty_targets", empty + "\n" + empty, 1],
    ["bare_empty", "running 0 tests", 1],
    ["ignored_only", "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out", 1],
    ["other_empty_runner", one + "\nNo test files found", 1],
    ["npm_banner_version", "> @codeterm/provider-claude@1.14.0 test\n674 passed, 0 failed", 0],
  ];
  try {
    for (const [label, output, expected] of cases) {
      const result = runQa(h, label, ["printf '%s\\n' " + shellQuote(output)]);
      assert.equal(result.status, expected, label + "\n" + result.stdout + result.stderr);
      assert.equal(result.stdout.includes("ZERO TESTS RAN"), expected === 1, label);
    }
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("qa_report_uses_runner_summaries_instead_of_incidental_zero_tests", () => {
  const h = makeHarness();
  const cases = [
    ["cronfail", "running 5 tests\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out", 0],
    ["catalogue", "scripts/example.test.mjs: 0 tests\nTest catalogue validation passed", 0],
    ["libtest_inventory", "inventory: 0 tests\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out", 0],
    ["nextest", "inventory: 0 tests\n     Summary [ 0.052s] 5 tests run: 5 passed, 0 skipped", 0],
    ["node", "# inventory: 0 tests\n# tests 5\n# pass 5\n# fail 0", 0],
    ["nextest_empty", "     Summary [ 0.052s] 0 tests run: 0 passed, 5 skipped", 1],
    ["node_empty", "# tests 0\n# pass 0\n# fail 0", 1],
    ["libtest_empty", "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out", 1],
  ];
  try {
    for (const [label, output, expected] of cases) {
      const result = runQa(h, label, ["printf '%s\\n' " + shellQuote(output)]);
      assert.equal(result.status, expected, label + "\n" + result.stdout + result.stderr);
      const reportRoot = path.join(h.dir, "reports");
      const runDir = readdirSync(reportRoot).find((name) => name.startsWith(label + "-"));
      const summary = readFileSync(path.join(reportRoot, runDir, "summary.txt"), "utf8");
      assert.equal(summary.includes("ZERO TESTS RAN"), expected === 1, label);
      assert.ok(summary.includes(`QA ${label}: ${expected === 0 ? "PASS (1/1 ok)" : "FAIL (0/1 ok)"}`), summary);
    }
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("qa_report_failure_includes_tail", () => {
  const h = makeHarness();
  try {
    const command = "i=1; while [ \"$i\" -le 30 ]; do printf '%s\\n' \"$i\"; i=$((i + 1)); done; exit 3";
    const result = runQa(h, "tail", [command], { QA_NOTIFY_TAB: "agent-tab" });
    assert.equal(result.status, 1);
    const body = records(h.parentRecord).at(-1).body;
    assert.match(body, /\[1\/1\] rc=3/);
    const lastLines = Array.from({ length: 25 }, (_, i) => String(i + 6)).join("\n") + "\n";
    assert.ok(body.includes(":\n" + lastLines), "summary carries exactly the last 25 lines");
    assert.ok(!body.includes("\n5\n"), "lines before the last 25 are not included");
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("qa_report_parent_env_survives_instance_exports", () => {
  const h = makeHarness();
  try {
    const command =
      "export PATH=" + shellQuote(h.devBin) +
      ":\"$PATH\"; export CODETERM_INSTANCE=devinst CODETERM_API_PORT=7690; true";
    const result = runQa(h, "parent-env", [command], {
      QA_NOTIFY_TAB: "parent-agent",
      CODETERM_INSTANCE: "memrerank",
      CODETERM_API_PORT: "7695",
      CODETERM_API_URL: "http://127.0.0.1:7685",
      CODETERM_API_TOKEN: "parent-token",
      CODETERM_SOCKET_PATH: "parent.sock",
      CODETERM_TAB_ID: "parent-tab",
    });
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].env.CODETERM_INSTANCE, null);
    assert.equal(sent[0].env.CODETERM_API_PORT, null);
    assert.equal(sent[0].env.CODETERM_API_URL, "http://127.0.0.1:7685");
    assert.equal(sent[0].env.CODETERM_API_TOKEN, "parent-token");
    assert.equal(sent[0].env.CODETERM_SOCKET_PATH, "parent.sock");
    assert.equal(sent[0].env.CODETERM_TAB_ID, "parent-tab");
    assert.equal(records(h.devRecord).length, 0);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_ready_sends_one_short_sha_receipt", () => {
  const h = makeHarness();
  try {
    const marker = path.join(h.dir, "ready-sent");
    const sha = "a".repeat(40);
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "dev_notify_ready_once " + shellQuote(marker) + " agent-tab named 7690 " + sha + " 4321 " + shellQuote(root),
    ].join("\n");
    const result = runBash(["-c", script], h.env);
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].body, `READY instance=named api_port=7690 build=${sha}\n`);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_failure_sends_stage_and_first_compiler_errors", () => {
  const h = makeHarness();
  try {
    const marker = path.join(h.dir, "failed-sent");
    const log = path.join(h.dir, "dev.log");
    writeFileSync(log, ["compile started", "error[E0432]: unresolved import `crate::missing`", "second compiler line", ...Array.from({ length: 40 }, (_, i) => String(i + 1))].join("\n") + "\n");
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "dev_notify_failure_once " + shellQuote(marker) + " agent-tab health named 7690 " + shellQuote(log) + " 7",
    ].join("\n");
    const result = runBash(["-c", script], h.env);
    assert.equal(result.status, 0, result.stderr);
    const body = records(h.parentRecord)[0].body;
    assert.match(body, /^FAILED stage=health rc=7 instance=named api_port=7690\n/);
    assert.match(body, /Compiler errors:\nerror\[E0432\]: unresolved import/);
    assert.doesNotMatch(body, /\n40\n/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_uses_parent_env_when_instance_set", () => {
  const h = makeHarness();
  try {
    const bodyFile = path.join(h.dir, "body.txt");
    writeFileSync(bodyFile, "file-backed report\n");
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "export CODETERM_INSTANCE=devinst CODETERM_API_PORT=7690 CODETERM_API_URL=http://127.0.0.1:7690 CODETERM_API_TOKEN=dev-token CODETERM_SOCKET_PATH=dev.sock CODETERM_DATA_DIR=/dev/data CODETERM_TAB_ID=dev-tab CODETERM_SESSION_ID=dev-session CODETERM_SESSION_BINDING_NONCE=dev-nonce CODETERM_WORKER_ID=dev-worker",
      "export PATH=" + shellQuote(h.devBin) + ":\"$PATH\"",
      "dev_notify_send parent-tab " + shellQuote(bodyFile),
    ].join("\n");
    const result = runBash(["-c", script], {
      ...h.env,
      CODETERM_INSTANCE: "memrerank",
      CODETERM_API_PORT: "7695",
      CODETERM_API_URL: "http://127.0.0.1:7685",
      CODETERM_API_TOKEN: "parent-token",
      CODETERM_SOCKET_PATH: "parent.sock",
      CODETERM_DATA_DIR: path.join(h.dir, "parent-data"),
      CODETERM_TAB_ID: "parent-tab",
      CODETERM_SESSION_ID: "parent-session",
      CODETERM_SESSION_BINDING_NONCE: "parent-nonce",
      CODETERM_WORKER_ID: "parent-worker",
    });
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].executable, h.parentCli);
    assert.equal(sent[0].env.CODETERM_INSTANCE, null);
    assert.equal(sent[0].env.CODETERM_API_PORT, null);
    assert.equal(sent[0].env.CODETERM_API_URL, "http://127.0.0.1:7685");
    assert.equal(sent[0].env.CODETERM_API_TOKEN, "parent-token");
    assert.equal(sent[0].env.CODETERM_SOCKET_PATH, "parent.sock");
    assert.equal(sent[0].env.CODETERM_DATA_DIR, path.join(h.dir, "parent-data"));
    assert.equal(sent[0].env.CODETERM_TAB_ID, "parent-tab");
    assert.equal(sent[0].env.CODETERM_SESSION_ID, "parent-session");
    assert.equal(sent[0].env.CODETERM_SESSION_BINDING_NONCE, "parent-nonce");
    assert.equal(sent[0].env.CODETERM_WORKER_ID, "parent-worker");
    assert.equal(records(h.devRecord).length, 0);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_sends_remote_receipts_through_codeterm_mesh", () => {
  const h = makeHarness();
  try {
    const bodyFile = path.join(h.dir, "remote-body.txt");
    writeFileSync(bodyFile, "remote report\n");
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "DEV_NOTIFY_TARGET_KIND=send",
      "DEV_NOTIFY_TARGET_MESH=paired-origin-id",
      "dev_notify_send origin-owner " + shellQuote(bodyFile),
    ].join("\n");
    const result = runBash(["-c", script], h.env);
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args, ["send", "--mesh", "paired-origin-id", "--tab", "origin-owner", "--from-file", bodyFile, "--system-context", "build"]);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_stream_waiter_ready_on_health_event", async () => {
  const h = makeHarness();
  try {
    const log = path.join(h.dir, "interactive.log");
    const marker = path.join(h.dir, "stream-sent");
    const tailPidFile = path.join(h.dir, "tail.pid");
    const streamSha = "b".repeat(40);
    writeFileSync(log, "");
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "DEV_NOTIFY_SENT_MARKER=" + shellQuote(marker),
      "CT_INSTANCE=stream-instance",
      "CT_PORT=7690",
      "TEST_LOG=" + shellQuote(log),
      "dev_notify_health_proof() {",
      "  if grep -q 'health-ok' \"$TEST_LOG\"; then",
      "    dev_notify_ready_once \"$DEV_NOTIFY_SENT_MARKER\" \"$1\" \"$CT_INSTANCE\" \"$CT_PORT\" " + streamSha + " 9876 " + shellQuote(root),
      "  else",
      "    return 1",
      "  fi",
      "}",
      "dev_notify_wait_stream " + shellQuote(log) + " 15 \"$$\" agent-tab",
    ].join("\n");
    const child = spawn(bash, ["-c", script], {
      cwd: root,
      env: {
        ...h.env,
        DEV_NOTIFY_TEST_MODE: "1",
        DEV_NOTIFY_TAIL_PID_FILE: tailPidFile,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
    const started = new Promise((resolve, reject) => {
      const onData = (chunk) => {
        stdout += chunk.toString();
        if (stdout.includes("DEV_NOTIFY_WAITER_STARTED")) resolve();
      };
      child.stdout.on("data", onData);
      child.once("error", reject);
      child.once("close", (code) => reject(new Error("waiter exited before stream setup (" + code + "): " + stderr)));
    });
    await started;
    appendFileSync(log, "health-ok\n");
    const [code] = await once(child, "close");
    assert.equal(code, 0, stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.ok(sent[0].body.startsWith("READY instance=stream-instance api_port=7690 build=" + streamSha));
    const tailPid = Number(readFileSync(tailPidFile, "utf8").trim());
    assert.ok(Number.isInteger(tailPid) && tailPid > 0);
    assert.throws(() => process.kill(tailPid, 0), { code: "ESRCH" });
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_stream_waiter_deadline", () => {
  const h = makeHarness();
  try {
    const log = path.join(h.dir, "quiet.log");
    const marker = path.join(h.dir, "deadline-sent");
    const tailBin = path.join(h.dir, "tail-bin");
    const tailArgs = path.join(h.dir, "tail-args");
    const tailPidFile = path.join(h.dir, "tail.pid");
    const fakeTail = path.join(tailBin, "tail");
    mkdirSync(tailBin, { recursive: true });
    writeFileSync(log, "");
    const logWriter = openSync(log, "a");
    writeFileSync(fakeTail,
      "#!/usr/bin/env bash\n" +
      "if [ \"$1\" = \"-F\" ]; then printf '%s\\n' \"$@\" > \"$TAIL_ARGS_FILE\"; fi\n" +
      "exec /usr/bin/tail \"$@\"\n",
      { mode: 0o755 },
    );
    chmodSync(fakeTail, 0o755);
    const script = [
      'export PATH="$(cd ' + shellQuote(tailBin) + ' && pwd):$PATH"',
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "DEV_NOTIFY_SENT_MARKER=" + shellQuote(marker),
      "CT_INSTANCE=quiet-instance",
      "CT_PORT=7690",
      "dev_notify_health_proof() { return 1; }",
      "dev_notify_wait_stream " + shellQuote(log) + " 2 \"$$\" agent-tab",
    ].join("\n");
    let result;
    try {
      result = runBash(["-c", script], {
        ...h.env,
        PATH: tailBin + path.delimiter + h.env.PATH,
        TAIL_ARGS_FILE: tailArgs,
        DEV_NOTIFY_TAIL_PID_FILE: tailPidFile,
      });
    } finally {
      closeSync(logWriter);
    }
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(readFileSync(tailArgs, "utf8").trim().split("\n"), ["-F", "-n", "+1", log]);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.match(sent[0].body, /^FAILED stage=deadline instance=quiet-instance api_port=7690\n/);
    const tailPid = Number(readFileSync(tailPidFile, "utf8").trim());
    assert.throws(() => process.kill(tailPid, 0), { code: "ESRCH" });
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_stream_waiter_tail_exit_reports_stream_failure", () => {
  const h = makeHarness();
  try {
    const log = path.join(h.dir, "stream.log");
    const marker = path.join(h.dir, "stream-sent");
    const tailBin = path.join(h.dir, "tail-bin");
    const tailArgs = path.join(h.dir, "tail-args");
    const fakeTail = path.join(tailBin, "tail");
    mkdirSync(tailBin, { recursive: true });
    writeFileSync(log, "");
    writeFileSync(fakeTail,
      "#!/usr/bin/env bash\n" +
      "if [ \"$1\" = \"-F\" ]; then printf '%s\\n' \"$@\" > \"$TAIL_ARGS_FILE\"; exit 17; fi\n" +
      "exec /usr/bin/tail \"$@\"\n",
      { mode: 0o755 },
    );
    chmodSync(fakeTail, 0o755);
    const script = [
      'export PATH="$(cd ' + shellQuote(tailBin) + ' && pwd):$PATH"',
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "DEV_NOTIFY_SENT_MARKER=" + shellQuote(marker),
      "CT_INSTANCE=stream-instance",
      "CT_PORT=7690",
      "dev_notify_wait_stream " + shellQuote(log) + " 10 \"$$\" agent-tab",
    ].join("\n");
    const env = {
      ...h.env,
      PATH: tailBin + path.delimiter + h.env.PATH,
      TAIL_ARGS_FILE: tailArgs,
    };
    const result = runBash(["-c", script], env);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(readFileSync(tailArgs, "utf8").trim().split("\n"), ["-F", "-n", "+1", log]);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.match(sent[0].body, /^FAILED stage=stream rc=17 instance=stream-instance api_port=7690\n/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_sh_parses_notify_tab_before_path_change", () => {
  const h = makeHarness();
  try {
    const env = {
      ...h.env,
      HOME: path.join(h.dir, "home"),
      CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY: "1",
    };
    mkdirSync(env.HOME, { recursive: true });
    const separated = runBash([buildPath, "dev", "--notify-tab", "agent-tab"], env);
    assert.equal(separated.status, 0, separated.stderr);
    assert.match(separated.stdout, /notify_tab=agent-tab/);
    assert.ok(separated.stdout.includes("parent_cli="));
    assert.ok(!separated.stdout.includes(path.join(env.HOME, ".domios-dev", "bin")));
    const equals = runBash([buildPath, "dev", "--notify-tab=agent-tab"], env);
    assert.equal(equals.status, 0, equals.stderr);
    assert.match(equals.stdout, /notify_tab=agent-tab/);
    const empty = runBash([buildPath, "dev", "--notify-tab="], env);
    assert.equal(empty.status, 2);
    assert.match(empty.stderr, /non-empty tab id/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_sh_notifies_parent_when_caller_sets_named_instance", () => {
  const h = makeHarness();
  try {
    const env = {
      ...h.env,
      HOME: path.join(h.dir, "home"),
      CODETERM_INSTANCE: "memrerank",
      CODETERM_API_PORT: "7695",
      CODETERM_API_URL: "http://127.0.0.1:7685",
      CODETERM_API_TOKEN: "parent-token",
      CODETERM_SOCKET_PATH: "parent.sock",
      CODETERM_DATA_DIR: path.join(h.dir, "parent-data"),
      CODETERM_TAB_ID: "parent-caller",
      CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY: "1",
      CODETERM_TEST_SEND_PARENT_REPORT: "1",
    };
    mkdirSync(env.HOME, { recursive: true });
    const result = runBash([buildPath, "dev", "--notify-tab", "agent-tab"], env);
    assert.equal(result.status, 0, result.stderr);
    assert.ok(result.stdout.includes("parent_cli="));
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].executable, h.parentCli);
    assert.equal(sent[0].env.CODETERM_INSTANCE, null);
    assert.equal(sent[0].env.CODETERM_API_PORT, null);
    assert.equal(sent[0].env.CODETERM_API_URL, "http://127.0.0.1:7685");
    assert.equal(sent[0].env.CODETERM_API_TOKEN, "parent-token");
    assert.equal(sent[0].env.CODETERM_SOCKET_PATH, "parent.sock");
    assert.equal(sent[0].env.CODETERM_DATA_DIR, path.join(h.dir, "parent-data"));
    assert.equal(sent[0].env.CODETERM_TAB_ID, "parent-caller");
    assert.equal(records(h.devRecord).length, 0);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

function writeFakeTool(dir, name, body) {
  const tool = path.join(dir, "parent-bin", name);
  writeFileSync(tool, "#!/usr/bin/env bash\n" + body + "\n", { mode: 0o755 });
  chmodSync(tool, 0o755);
  if (name === "rustfmt") {
    writeFakeTool(dir, "git", [
      "case \"$*\" in",
      "  'ls-files -z -- src-tauri/**.rs cli/**.rs') printf 'src-tauri/src/lib.rs\\0'; exit 0 ;;",
      "esac",
      "exec " + shellQuote(realGit) + " \"$@\"",
    ].join("\n"));
  }
}

function writeFakeCargo(dir, body) {
  writeFakeTool(dir, "cargo", body);
}

test("build_notify_tab_success_sends_one_ok_receipt", () => {
  const h = makeHarness();
  try {
    writeFakeTool(h.dir, "rustfmt", "exit 0");
    const result = runBash([buildPath, "fmt-check", "--notify-tab", "agent-tab"], h.env);
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args.slice(0, 3), ["send", "--tab", "agent-tab"]);
    assert.match(sent[0].body, /^OK label=fmt-check/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_notify_tab_failure_names_step_first_error_and_log", () => {
  const h = makeHarness();
  try {
    writeFakeTool(h.dir, "rustfmt", "echo 'Diff in src/foo.rs:1:'; exit 1");
    const started = Date.now();
    const result = runBash([buildPath, "fmt-check", "--notify-tab", "agent-tab"], h.env);
    assert.equal(result.status, 1);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.ok(sent[0].sentAt - started < 30000, "format failure receipt must arrive within the failure deadline");
    assert.match(sent[0].body, /EARLY FAILURE label=fmt-check step=fmt-check/);
    assert.match(sent[0].body, /First failure: Diff in src\/foo\.rs/);
    assert.match(sent[0].body, /Log: .*codeterm-build-fmt-check/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_dev_defaults_to_the_inspected_parent_and_sends_ready_once", () => {
  const h = makeHarness();
  try {
    const tabModel = path.join(h.dir, "tab-model.json");
    writeFileSync(tabModel, JSON.stringify([{ id: "dev-shell", parent_tab_id: "live-owner" }]));
    const result = runBash([buildPath, "dev"], {
      ...h.env,
      CODETERM_TAB_ID: "dev-shell",
      CODETERM_PARENT_TAB_ID: "stale-parent-env",
      BUILD_NOTIFICATION_TAB_MODEL: tabModel,
      CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY: "1",
      CODETERM_TEST_SEND_PARENT_REPORT: "1",
    });
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args.slice(0, 3), ["send", "--tab", "live-owner"]);
    assert.match(sent[0].body, /^READY instance=reportfixture api_port=17695 build=a{40}\n$/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_dev_uses_one_self_notification_for_ready_without_a_parent", () => {
  const h = makeHarness();
  try {
    const tabModel = path.join(h.dir, "tab-model.json");
    writeFileSync(tabModel, JSON.stringify([{ id: "standalone-shell" }]));
    const result = runBash([buildPath, "dev"], {
      ...h.env,
      CODETERM_TAB_ID: "standalone-shell",
      BUILD_NOTIFICATION_TAB_MODEL: tabModel,
      CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY: "1",
      CODETERM_TEST_SEND_PARENT_REPORT: "1",
    });
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].args[0], "notify");
    assert.ok(sent[0].args.includes("--from-pad"));
    assert.ok(sent[0].args.includes("--level") && sent[0].args[sent[0].args.indexOf("--level") + 1] === "verbose");
    assert.match(sent[0].body, /^READY instance=reportfixture api_port=17695 build=a{40}\n$/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_long_verb_reports_one_failure_to_the_live_parent_by_default", () => {
  const h = makeHarness();
  try {
    writeFakeTool(h.dir, "rustfmt", "echo 'Diff in src/failing.rs:1:'; exit 1");
    const tabModel = path.join(h.dir, "tab-model.json");
    writeFileSync(tabModel, JSON.stringify([{
      id: "build-shell",
      parent_tab_id: "live-orchestrator",
      spawned_by_tab_id: "launcher-agent",
      team_id: "team-fixture",
    }]));
    const result = runBash([buildPath, "fmt-check"], {
      ...h.env,
      CODETERM_TAB_ID: "build-shell",
      CODETERM_PARENT_TAB_ID: "stale-env-parent",
      BUILD_NOTIFICATION_TAB_MODEL: tabModel,
    });
    assert.equal(result.status, 1, result.stdout + result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1, result.stdout + result.stderr + JSON.stringify(sent));
    assert.deepEqual(sent[0].args.slice(0, 3), ["send", "--tab", "live-orchestrator"]);
    assert.match(sent[0].body, /EARLY FAILURE label=fmt-check/);
    assert.match(sent[0].body, /First failure: Diff in src\/failing\.rs/);
    assert.match(sent[0].body, /Log: .*codeterm-build-fmt-check/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_long_verb_uses_one_failure_notification_when_the_shell_has_no_parent", () => {
  const h = makeHarness();
  try {
    writeFakeTool(h.dir, "rustfmt", "echo 'Diff in src/failing.rs:1:'; exit 1");
    const tabModel = path.join(h.dir, "tab-model.json");
    writeFileSync(tabModel, JSON.stringify([{ id: "standalone-shell" }]));
    const result = runBash([buildPath, "fmt-check"], {
      ...h.env,
      CODETERM_TAB_ID: "standalone-shell",
      BUILD_NOTIFICATION_TAB_MODEL: tabModel,
    });
    assert.equal(result.status, 1, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].args[0], "notify");
    assert.ok(sent[0].args.includes("--level") && sent[0].args[sent[0].args.indexOf("--level") + 1] === "failure");
    assert.ok(sent[0].args.includes("--from-pad"));
    assert.match(sent[0].body, /EARLY FAILURE label=fmt-check/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_notify_tab_reports_gateway_busy_and_env_equivalent", () => {
  const h = makeHarness();
  try {
    writeFakeTool(h.dir, "rustfmt", "exit 75");
    const result = runBash([buildPath, "fmt-check"], { ...h.env, BUILD_NOTIFY_TAB: "env-tab" });
    assert.equal(result.status, 75);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args.slice(0, 3), ["send", "--tab", "env-tab"]);
    assert.match(sent[0].body, /gateway refused \(exit 75\)/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("build_notify_tab_requires_a_value", () => {
  const h = makeHarness();
  try {
    const result = runBash([buildPath, "check", "--notify-tab"], h.env);
    assert.equal(result.status, 2);
    assert.match(result.stderr, /non-empty tab id/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("check_queue_notifies_invalid_revision_without_reaching_cargo", () => {
  const h = makeHarness();
  try {
    writeFakeCargo(h.dir, "echo 'unexpected cargo invocation' >&2; exit 99");
    const result = runBash([buildPath, "check-queue", "does-not-exist-notify-fixture", "--notify-tab", "agent-tab"], h.env);
    assert.notEqual(result.status, 0);
    assert.notEqual(result.status, 2, result.stderr);
    assert.doesNotMatch(result.stdout + result.stderr, /unexpected cargo invocation/);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args.slice(0, 3), ["send", "--tab", "agent-tab"]);
    assert.match(sent[0].body, /EARLY FAILURE label=check-queue/);
    assert.match(sent[0].body, /does-not-exist-notify-fixture/);
    assert.match(sent[0].body, /Log: .*codeterm-build-check-queue/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test('powershell launcher forwards checks without writing shared Git configuration', { skip: process.platform !== 'win32' }, () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'powershell-build-forward-'));
  try {
    const launcher = path.join(directory, 'build.ps1');
    writeFileSync(launcher, readFileSync(path.join(root, 'build.ps1'), 'utf8'));
    writeFileSync(path.join(directory, 'build.sh'), '#!/usr/bin/env bash\nprintf "FORWARDED %s\\n" "$*"\n');
    const quoted = launcher.replaceAll("'", "''");
    const result = spawnSync('powershell.exe', ['-NoProfile', '-Command', `function global:git { throw 'unexpected shared config write' }; & '${quoted}' check-queue opaque-sha -NotifyTab opaque-tab`], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /FORWARDED check-queue opaque-sha --notify-tab opaque-tab/);
    assert.doesNotMatch(result.stdout + result.stderr, /unexpected shared config write/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('nextest verbs use the shared notification wrapper for validation failures', () => {
  for (const [verb, selector] of [['nextest', 'unknown-notify-scope'], ['nextest-archive', 'does-not-exist-notify-fixture']]) {
    const h = makeHarness();
    try {
      writeFakeCargo(h.dir, "echo 'unexpected cargo invocation' >&2; exit 99");
      const result = runBash([buildPath, verb, selector, '--notify-tab', 'agent-tab'], h.env);
      assert.notEqual(result.status, 0);
      assert.doesNotMatch(result.stdout + result.stderr, /unexpected cargo invocation/);
      const sent = records(h.parentRecord);
      assert.equal(sent.length, 1);
      assert.match(sent[0].body, new RegExp(`EARLY FAILURE label=${verb}`));
      assert.ok(sent[0].body.includes(selector));
    } finally { rmSync(h.dir, { recursive: true, force: true }); }
  }
});

test("early_failure_receipt_names_the_guard_failure_not_gateway_bookkeeping", () => {
  const h = makeHarness();
  try {
    const log = path.join(h.dir, "preflight.log");
    const captured = [
      "==> provider boundary",
      "\u001b[31mProvider boundary FAIL: added occurrence src/agent/x.rs:\"codex\" (1 > 0)\u001b[0m",
      "[build-gateway] sccache server at job end: pid=4242 started=2026-10-20T05:00:00Z",
      "",
    ].join("\n");
    const firstFailure = (contents) => {
      writeFileSync(log, contents);
      const result = runBash(["-c", `source ${shellQuote(helperPath)}\nreport_first_failure ${shellQuote(log)}`], h.env);
      assert.equal(result.status, 0, result.stderr);
      return result.stdout.trim();
    };
    assert.equal(firstFailure(captured), 'Provider boundary FAIL: added occurrence src/agent/x.rs:"codex" (1 > 0)');
    assert.equal(firstFailure("thread 'main' panicked at src/lib.rs:3:5:\n[build-gateway] done\n"), "thread 'main' panicked at src/lib.rs:3:5:");
    assert.equal(firstFailure("test result: FAILED. 3 passed; 1 failed\n"), "test result: FAILED. 3 passed; 1 failed");
    assert.equal(firstFailure("  ✗ FAILED: src/a.test.ts\n"), "✗ FAILED: src/a.test.ts");
    assert.equal(firstFailure("plain cause\n[build-gateway] sccache server at job end: x\n"), "plain cause");

    const marker = path.join(h.dir, "early-sent");
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      `report_run_stream ${shellQuote(marker)} agent-tab release-preflight guards ${shellQuote(path.join(h.dir, "stream.log"))} ` +
        "bash -c " + shellQuote("echo 'Provider boundary FAIL: added occurrence'; echo '[build-gateway] sccache server at job end: x'; exit 1"),
    ].join("\n");
    const result = runBash(["-c", script], h.env);
    assert.equal(result.status, 1, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1, JSON.stringify(sent));
    assert.match(sent[0].body, /First failure: Provider boundary FAIL: added occurrence\n/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("prebuilt_dev_survives_a_failed_readiness_notice_after_proving_readiness", () => {
  const h = makeHarness();
  try {
    const launchCase = "  dev-prebuilt)\n    CT_SPLIT_DEV=1\n";
    const fixture = readFileSync(h.env.BUILD_NOTIFICATION_FIXTURE_PATH, "utf8");
    assert.ok(fixture.includes(launchCase), "build fixture must stub the prebuilt launch");
    writeFileSync(h.env.BUILD_NOTIFICATION_FIXTURE_PATH, fixture.replace(launchCase, launchCase + '    source "$BUILD_NOTIFICATION_PREBUILT_STUBS"\n'));
    const killLog = path.join(h.dir, "kill.log");
    const stubs = path.join(h.dir, "prebuilt-stubs.sh");
    writeFileSync(stubs, [
      "run_prebuilt_preflight() { :; }",
      "stop_dev_before_replacement() { echo stop >> " + shellQuote(killLog) + "; }",
      "kill_dev() { echo kill >> " + shellQuote(killLog) + "; }",
      "configure_dev_cargo() { :; }",
      "run_outside_tab_job() { :; }",
      "dev_health_status() { echo '{}'; }",
      "validate_dev_health() { printf '%s\\n' \"$1\"; }",
      "dev_listener_pid() { echo 4321; }",
      "dev_app_path() { echo app; }",
      "dev_cli_path() { echo cli; }",
      "dev_provenance_record() { echo record; }",
      "node() { :; }",
      "",
    ].join("\n"));
    const home = path.join(h.dir, "home");
    mkdirSync(home, { recursive: true });
    const result = runBash([buildPath, "dev-prebuilt", "--wait", "--notify-tab", "agent-tab"], {
      ...h.env,
      HOME: home,
      DEV_FRONT_EXTERNAL: "1",
      CODETERM_DEV_INSTANCE: "notifyfx",
      CODETERM_DEV_API_PORT: "7695",
      BUILD_NOTIFICATION_PREBUILT_STUBS: stubs,
      BUILD_NOTIFICATION_SEND_EXIT: "1",
    });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const output = result.stdout + result.stderr;
    assert.match(output, /Prebuilt dev ready: build [0-9a-f]{8}/);
    assert.match(output, /readiness notice to tab agent-tab failed; DEV keeps running/);
    assert.doesNotMatch(output, /prebuilt dev launch failed/);
    assert.deepEqual(readFileSync(killLog, "utf8").trim().split("\n"), ["stop"]);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1, JSON.stringify(sent));
    assert.match(sent[0].body, /^READY instance=notifyfx api_port=7695 build=[0-9a-f]{40}\n$/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_signs_anonymous_build_reports_as_system", () => {
  const h = makeHarness();
  try {
    const bodyFile = path.join(h.dir, "anonymous-report.md");
    writeFileSync(bodyFile, "opaque build result");
    const result = runBash(["-c", 'source "$1"; dev_notify_capture_parent; dev_notify_send agent-tab "$2"', "_", helperPath, bodyFile], h.env);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].env.CODETERM_TAB_ID, null);
    assert.deepEqual(sent[0].args, ["send", "--tab", "agent-tab", "--from-file", bodyFile, "--system-context", "build"]);
    assert.equal(sent[0].body, "opaque build result");
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("ease_tab_build_report_uses_tab_provenance_without_system_claim", () => {
  const h = makeHarness();
  try {
    const bodyFile = path.join(h.dir, "tab-report.md");
    writeFileSync(bodyFile, "OK build completed\n");
    const result = runBash(["-c", 'source "$1"; dev_notify_capture_parent; dev_notify_send agent-tab "$2"', "_", helperPath, bodyFile], {
      ...h.env, CODETERM_TAB_ID: "utility-tab", CODETERM_SESSION_BINDING_NONCE: "utility-nonce",
    });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(result.stderr, "");
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args, ["send", "--tab", "agent-tab", "--from-file", bodyFile]);
    assert.equal(sent[0].env.CODETERM_TAB_ID, "utility-tab");
    assert.equal(sent[0].env.CODETERM_SESSION_BINDING_NONCE, "utility-nonce");
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});
