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
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const bash = existsSync("/bin/bash") ? "/bin/bash" : "bash";
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
];

function shellQuote(value) {
  return "'" + String(value).replace(/'/g, "'\\''") + "'";
}

function cleanEnv(extra = {}) {
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (key.startsWith("CODETERM_")) delete env[key];
  }
  for (const key of ["QA_NOTIFY_TAB", "QA_REPORT_DIR", "DEV_NOTIFY_TEST_MODE", "DEV_NOTIFY_TAIL_PID_FILE"]) {
    delete env[key];
  }
  return { ...env, ...extra };
}

function writeFakeCli(cliPath, recordPath) {
  mkdirSync(path.dirname(cliPath), { recursive: true });
  const source =
    "#!/usr/bin/env node\n" +
    "const fs = require('node:fs');\n" +
    "const args = process.argv.slice(2);\n" +
    "const fromFile = args.indexOf('--from-file');\n" +
    "const bodyFile = fromFile >= 0 ? args[fromFile + 1] : null;\n" +
    "const keys = " + JSON.stringify(connectionKeys) + ";\n" +
    "const env = {};\n" +
    "for (const key of keys) env[key] = process.env[key] ?? null;\n" +
    "const record = { executable: process.argv[1], args, bodyFile, body: bodyFile ? fs.readFileSync(bodyFile, 'utf8') : null, env };\n" +
    "fs.appendFileSync(" + JSON.stringify(recordPath) + ", JSON.stringify(record) + '\\n');\n";
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
  writeFakeCli(parentCli, parentRecord);
  writeFakeCli(devCli, devRecord);
  return {
    dir,
    parentBin,
    devBin,
    parentRecord,
    devRecord,
    parentCli,
    devCli,
    env: cleanEnv({ PATH: parentBin + path.delimiter + (process.env.PATH ?? "") }),
  };
}

function records(recordPath) {
  if (!existsSync(recordPath)) return [];
  return readFileSync(recordPath, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function runBash(args, env, cwd = root) {
  return spawnSync(bash, args, { cwd, env, encoding: "utf8" });
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
      CODETERM_DATA_DIR: "/parent/data",
      CODETERM_TAB_ID: "parent-caller",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /QA smoke: PASS \(2\/2 ok\)/);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.deepEqual(sent[0].args, ["send", "--tab", "agent-tab", "--from-file", sent[0].bodyFile]);
    assert.match(sent[0].body, /test result: ok\. 3 passed; 0 failed/);
    assert.match(sent[0].body, /QA smoke: PASS \(2\/2 ok\)/);
    assert.equal(sent[0].env.CODETERM_INSTANCE, "parent");
    assert.equal(sent[0].env.CODETERM_API_PORT, "7685");
    assert.equal(sent[0].env.CODETERM_API_URL, "http://127.0.0.1:7685");
    assert.equal(sent[0].env.CODETERM_API_TOKEN, "parent-token");
    assert.equal(sent[0].env.CODETERM_SOCKET_PATH, "parent.sock");
    assert.equal(sent[0].env.CODETERM_DATA_DIR, "/parent/data");
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
    assert.equal(records(h.parentRecord).length, 1);
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

test("qa_report_failure_includes_tail", () => {
  const h = makeHarness();
  try {
    const command = "i=1; while [ \"$i\" -le 30 ]; do printf '%s\\n' \"$i\"; i=$((i + 1)); done; exit 3";
    const result = runQa(h, "tail", [command], { QA_NOTIFY_TAB: "agent-tab" });
    assert.equal(result.status, 1);
    const body = records(h.parentRecord)[0].body;
    assert.match(body, /\[1\/1\] rc=3/);
    assert.ok(body.endsWith(Array.from({ length: 25 }, (_, i) => String(i + 6)).join("\n") + "\n"));
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
      CODETERM_INSTANCE: "parent",
      CODETERM_API_PORT: "7685",
      CODETERM_API_TOKEN: "parent-token",
      CODETERM_TAB_ID: "parent-tab",
    });
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].env.CODETERM_INSTANCE, "parent");
    assert.equal(sent[0].env.CODETERM_API_PORT, "7685");
    assert.equal(sent[0].env.CODETERM_TAB_ID, "parent-tab");
    assert.equal(records(h.devRecord).length, 0);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_ready_sends_sha", () => {
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
    assert.ok(sent[0].body.startsWith("READY instance=named api_port=7690 build=" + sha + " listener_pid=4321"));
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_failure_sends_stage_and_tail", () => {
  const h = makeHarness();
  try {
    const marker = path.join(h.dir, "failed-sent");
    const log = path.join(h.dir, "dev.log");
    writeFileSync(log, Array.from({ length: 40 }, (_, i) => String(i + 1)).join("\n") + "\n");
    const script = [
      "source " + shellQuote(helperPath),
      "dev_notify_capture_parent",
      "dev_notify_failure_once " + shellQuote(marker) + " agent-tab health named 7690 " + shellQuote(log) + " 7",
    ].join("\n");
    const result = runBash(["-c", script], h.env);
    assert.equal(result.status, 0, result.stderr);
    const body = records(h.parentRecord)[0].body;
    assert.match(body, /^FAILED stage=health rc=7 instance=named api_port=7690\n/);
    assert.ok(body.endsWith(Array.from({ length: 25 }, (_, i) => String(i + 16)).join("\n") + "\n"));
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
      CODETERM_INSTANCE: "parent",
      CODETERM_API_PORT: "7685",
      CODETERM_TAB_ID: "parent-tab",
      CODETERM_SESSION_ID: "parent-session",
      CODETERM_SESSION_BINDING_NONCE: "parent-nonce",
      CODETERM_WORKER_ID: "parent-worker",
    });
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].executable, h.parentCli);
    assert.equal(sent[0].env.CODETERM_INSTANCE, "parent");
    assert.equal(sent[0].env.CODETERM_API_PORT, "7685");
    assert.equal(sent[0].env.CODETERM_API_URL, null);
    assert.equal(sent[0].env.CODETERM_API_TOKEN, null);
    assert.equal(sent[0].env.CODETERM_SOCKET_PATH, null);
    assert.equal(sent[0].env.CODETERM_DATA_DIR, null);
    assert.equal(sent[0].env.CODETERM_TAB_ID, "parent-tab");
    assert.equal(sent[0].env.CODETERM_SESSION_ID, "parent-session");
    assert.equal(sent[0].env.CODETERM_SESSION_BINDING_NONCE, "parent-nonce");
    assert.equal(sent[0].env.CODETERM_WORKER_ID, "parent-worker");
    assert.equal(records(h.devRecord).length, 0);
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
    writeFileSync(log, "");
    const logWriter = openSync(log, "a");
    const script = [
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
      result = runBash(["-c", script], h.env);
    } finally {
      closeSync(logWriter);
    }
    assert.equal(result.status, 0, result.stderr);
    const sent = records(h.parentRecord);
    assert.equal(sent.length, 1);
    assert.match(sent[0].body, /^FAILED stage=deadline instance=quiet-instance api_port=7690\n/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("dev_notify_stream_waiter_eof_reports_stream_failure", () => {
  const h = makeHarness();
  try {
    const log = path.join(h.dir, "stream.log");
    const marker = path.join(h.dir, "stream-sent");
    const tailBin = path.join(h.dir, "tail-bin");
    const tailArgs = path.join(h.dir, "tail-args");
    const fakeTail = path.join(tailBin, "tail");
    mkdirSync(tailBin, { recursive: true });
    writeFileSync(log, "");
    writeFileSync(fakeTail, "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > \"$TAIL_ARGS_FILE\"\nexit 17\n", { mode: 0o755 });
    chmodSync(fakeTail, 0o755);
    const script = [
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
    assert.ok(separated.stdout.includes("parent_cli=" + h.parentCli));
    assert.ok(!separated.stdout.includes(path.join(env.HOME, ".codeterm-dev", "bin")));
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
