import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter, once } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { resolveBash, runTsUnitTestMain, runTsUnitTests } from "./run-ts-unit-tests.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function runBash(args, env) {
  const bash = resolveBash();
  return new Promise((resolve, reject) => {
    const child = spawn(bash, args, {
      cwd: root,
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8").on("data", (chunk) => {
      stderr += chunk;
    });
    child.once("error", reject);
    child.once("close", (code, signal) => resolve({ code: code ?? 1, signal, stdout, stderr }));
  });
}

function runShellRunner(env) {
  return runBash(["scripts/ts-unit-tests.sh"], env);
}

function runCappedNodeEntrypoint(env) {
  return runBash(
    ["-lc", "timeout --signal=TERM --kill-after=5s 20s node scripts/run-ts-unit-tests.mjs"],
    env,
  );
}

function runOuterCappedNpmUnitGate(env) {
  return runBash(
    ["-lc", "timeout --signal=TERM --kill-after=5s 20s npm run test:unit"],
    env,
  );
}

async function closeServer(server) {
  if (!server.listening) return;
  const closed = once(server, "close");
  server.close();
  await closed;
}

async function createFixtureDirectory(t) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "codeterm-ts-runner-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

test("uses bash from PATH outside Windows", () => {
  assert.equal(resolveBash({ platform: "linux" }), "bash");
});

test("uses Git Bash from Program Files on Windows", () => {
  const bash = "C:\\Program Files\\Git\\bin\\bash.exe";
  assert.equal(
    resolveBash({ platform: "win32", env: { ProgramFiles: "C:\\Program Files" }, exists: (file) => file === bash }),
    bash,
  );
});

test("accepts an explicit Windows Bash path", () => {
  const bash = "D:\\tools\\bash.exe";
  assert.equal(resolveBash({ platform: "win32", env: { CODETERM_BASH: bash }, exists: () => true }), bash);
});

test("does not attempt a missing Windows Bash executable", () => {
  assert.equal(resolveBash({ platform: "win32", env: {}, exists: () => false }), undefined);
});

test("runs the existing shell runner through the resolved Windows Bash", async () => {
  const bash = "C:\\Program Files\\Git\\bin\\bash.exe";
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.kill = () => {};
  const signalSource = new EventEmitter();
  const env = { ProgramFiles: "C:\\Program Files" };
  const code = runTsUnitTests({
    platform: "win32",
    env,
    monitorDeadline: false,
    signalSource,
    exists: (file) => file === bash,
    spawnProcess(command, args, options) {
      assert.equal(command, bash);
      assert.deepEqual(args, [
        "-c",
        'printf "__CODETERM_TS_UNIT_SUPERVISOR_PID__=%s\\n" "$$"; exec scripts/ts-unit-tests.sh',
      ]);
      assert.deepEqual(options.stdio, ["ignore", "pipe", "inherit"]);
      assert.equal(options.env, env);
      queueMicrotask(() => child.emit("close", 0, null));
      return child;
    },
  });
  assert.equal(await code, 0);
});

test("forwards termination to the shell runner", async () => {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  const signalSource = new EventEmitter();
  let forwardedSignal;
  child.kill = (signal) => {
    forwardedSignal = signal;
    queueMicrotask(() => child.emit("close", 1, null));
  };

  const result = runTsUnitTests({
    platform: "linux",
    monitorDeadline: false,
    signalSource,
    spawnProcess: () => child,
  });
  signalSource.emit("SIGTERM");

  assert.equal(await result, 1);
  assert.equal(forwardedSignal, "SIGTERM");
});

test("uses the Git Bash supervisor pid for Windows termination", async () => {
  const bash = "C:\\Program Files\\Git\\bin\\bash.exe";
  const child = new EventEmitter();
  const supervisorPid = new PassThrough();
  const signalSource = new EventEmitter();
  const sent = [];
  let hardKilled = false;
  child.stdout = supervisorPid;
  child.kill = () => hardKilled = true;

  const result = runTsUnitTests({
    platform: "win32",
    monitorDeadline: false,
    signalSource,
    env: { ProgramFiles: "C:\\Program Files" },
    exists: (file) => file === bash,
    spawnProcess: () => child,
    spawnSignalProcess(command, args, options) {
      sent.push({ command, args, options });
      const sender = new EventEmitter();
      queueMicrotask(() => {
        sender.emit("exit", 0);
        child.emit("close", 1, null);
      });
      return sender;
    },
  });
  supervisorPid.write("__CODETERM_TS_UNIT_SUPERVISOR_PID__=4098\n");
  signalSource.emit("SIGTERM");

  assert.equal(await result, 1);
  assert.deepEqual(sent[0].command, bash);
  assert.deepEqual(sent[0].args, ["-c", "kill -TERM -- 4098"]);
  assert.equal(sent[0].options.stdio, "ignore");
  assert.equal(hardKilled, false);
});

test("entrypoint exits non-zero when the runner reports a deadline failure", async () => {
  const exitCodes = [];
  await runTsUnitTestMain({
    runTests: async () => 1,
    exitProcess: (code) => exitCodes.push(code),
  });
  assert.deepEqual(exitCodes, [1]);
});

test("reaps a completed worker before an older blocked worker", async (t) => {
  const directory = await createFixtureDirectory(t);
  const fixtureSource = `
import net from "node:net";
import path from "node:path";
import { once } from "node:events";

async function main() {
  const name = path.basename(process.argv[1]).split(".")[0];
  const socket = net.createConnection({
    host: "127.0.0.1",
    port: Number(process.env.RUNNER_TEST_PORT),
  });
  await once(socket, "connect");
  socket.write(name + "\\n");
  if (name === "slow") {
    await once(socket, "data");
  }
  socket.end();
  await once(socket, "close");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
`;
  const files = ["slow.test.ts", "fast.test.ts", "third.test.ts"];
  await Promise.all(files.map((file) => writeFile(path.join(directory, file), fixtureSource)));
  const listFile = path.join(directory, "tests.txt");
  await writeFile(listFile, files.map((file) => path.join(directory, file)).join("\n"));

  const server = net.createServer();
  t.after(() => closeServer(server));
  const connections = [];
  let slowSocket;
  let releasePending = false;
  server.on("connection", (socket) => {
    socket.setEncoding("utf8");
    socket.once("data", (name) => {
      const normalized = name.trim();
      connections.push(normalized);
      if (normalized === "slow") {
        slowSocket = socket;
        if (releasePending) slowSocket.write("release\n");
      }
      if (normalized === "third") {
        if (slowSocket) slowSocket.write("release\n");
        else releasePending = true;
      }
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  const result = await runShellRunner({
    RUNNER_TEST_PORT: String(server.address().port),
    TS_UNIT_JOBS: "2",
    TS_UNIT_TIMEOUT_SECONDS: "10",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "20",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
    TS_UNIT_TRACE: "1",
  });

  assert.equal(result.code, 0, result.stdout + result.stderr);
  assert.deepEqual([...connections].sort(), ["fast", "slow", "third"]);
  assert(connections.indexOf("third") > connections.indexOf("fast"), connections);
  const fastCompleted = result.stdout.indexOf("[ts-unit trace] completed index=1");
  const thirdLaunched = result.stdout.indexOf("[ts-unit trace] launched index=2");
  const slowCompleted = result.stdout.indexOf("[ts-unit trace] completed index=0");
  assert(fastCompleted >= 0, result.stdout);
  assert(thirdLaunched > fastCompleted, result.stdout);
  assert(slowCompleted > thirdLaunched, result.stdout);
  t.diagnostic("trace: completed index=1 → launched index=2 → completed index=0");
});

test("real entrypoint completes the first four chat-core files at width four", async (t) => {
  const directory = await createFixtureDirectory(t);
  const files = [
    "packages/chat-core/src/dedupe.test.ts",
    "packages/chat-core/src/markers.test.ts",
    "packages/chat-core/src/merge.test.ts",
    "packages/chat-core/src/sessionReset.test.ts",
  ];
  const listFile = path.join(directory, "tests.txt");
  await writeFile(listFile, `${files.join("\n")}\n`);

  const result = await runCappedNodeEntrypoint({
    TS_UNIT_JOBS: "4",
    TS_UNIT_TIMEOUT_SECONDS: "60",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "10",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
    TS_UNIT_TRACE: "1",
  });

  assert.equal(result.code, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /TS unit timing: 4 completed file\(s\), TS_UNIT_JOBS=4/);
  assert.match(result.stdout, /✓ all 4 TS unit test file\(s\) green/);
  for (const file of files) {
    assert.match(result.stdout, new RegExp(`════ ${file.replaceAll("/", "\\/")} \\((\\d+)ms\\) ════`));
  }
  const durations = [...result.stdout.matchAll(/\((\d+)ms\) ════/g)]
    .map((match) => Number(match[1]));
  assert.equal(durations.length, 4, result.stdout);
  assert(durations.every((duration) => duration < 2_000), result.stdout);
  t.diagnostic(`first-four wall times: ${durations.join(", ")}ms`);
});

async function verifySuiteDeadline(t, forcePortable, runGate = runShellRunner) {
  const directory = await createFixtureDirectory(t);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  const connected = new Promise((resolve) => connectedResolve = resolve);
  const closed = new Promise((resolve) => closedResolve = resolve);
  server.once("connection", (socket) => {
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("close", closedResolve);
    connectedResolve();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  const deadlineFile = path.join(directory, "deadline.test.ts");
  const neverStartedFile = path.join(directory, "never-started.test.ts");
  const grandchildSource = `
const net = require("node:net");
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("connect", () => socket.write("connected\\n"));
`;
  await writeFile(deadlineFile, `
import { spawn } from "node:child_process";
async function main() {
  const child = spawn(process.execPath, ["-e", ${JSON.stringify(grandchildSource)}], {
    env: process.env,
    stdio: "ignore",
  });
  await new Promise((resolve) => child.once("exit", resolve));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
`);
  await writeFile(neverStartedFile, `throw new Error("must not start");\n`);
  const listFile = path.join(directory, "tests.txt");
  await writeFile(listFile, `${deadlineFile}\n${neverStartedFile}\n`);

  const run = runGate({
    CODETERM_FORCE_PORTABLE_TIMEOUT: forcePortable ? "1" : "",
    RUNNER_TEST_PORT: String(server.address().port),
    TS_UNIT_JOBS: "1",
    TS_UNIT_TIMEOUT_SECONDS: "20",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "1",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
    TS_UNIT_TRACE: "1",
  });
  await connected;
  const result = await run;
  await closed;

  assert.equal(result.code, 1, result.stdout + result.stderr);
  assert.match(result.stdout, /TS UNIT SUITE INCOMPLETE/);
  assert.match(result.stdout, /PASSED AT DEADLINE \(0\)/);
  assert.match(result.stdout, /FAILED AT DEADLINE \(0\)/);
  assert.match(result.stdout, /STILL RUNNING AT DEADLINE \(1\)/);
  assert.match(result.stdout, /deadline\.test\.ts/);
  assert.match(result.stdout, /NEVER STARTED \(1\)/);
  assert.match(result.stdout, /never-started\.test\.ts/);
  assert.match(result.stdout, /✗ TS unit suite did not complete/);
}

test("GNU timeout suite deadline reports every state and kills descendants", (t) =>
  verifySuiteDeadline(t, false));

test("portable timeout suite deadline reports every state and kills descendants", (t) =>
  verifySuiteDeadline(t, true));

test("real capped npm topology returns a summary and leaves no descendants", async (t) => {
  const directory = await createFixtureDirectory(t);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  let workerPid;
  const connected = new Promise((resolve) => connectedResolve = resolve);
  const closed = new Promise((resolve) => closedResolve = resolve);
  server.once("connection", (socket) => {
    socket.setEncoding("utf8");
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("data", (data) => {
      workerPid = Number(data.trim());
      connectedResolve();
    });
    socket.once("close", closedResolve);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  const blockedFile = path.join(directory, "outer-timeout.test.ts");
  await writeFile(blockedFile, `
import net from "node:net";
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("connect", () => socket.write(String(process.pid) + "\\n"));
socket.on("error", () => {});
`);
  const listFile = path.join(directory, "tests.txt");
  await writeFile(listFile, `${blockedFile}\n`);

  const run = runOuterCappedNpmUnitGate({
    TS_UNIT_JOBS: "1",
    TS_UNIT_TIMEOUT_SECONDS: "20",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "20",
    CODETERM_TS_UNIT_ENTRYPOINT_TEST_DEADLINE_SECONDS: "6",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
    RUNNER_TEST_PORT: String(server.address().port),
    TS_UNIT_TRACE: "1",
  });
  await connected;
  const result = await run;
  await closed;
  t.diagnostic(`outer gate exited code=${result.code}`);
  const bashPidMatch = result.stdout.match(/\[ts-unit trace\] bash process pid=(\d+)/);
  const supervisorPidMatch = result.stdout.match(/\[ts-unit trace\] bash supervisor pid=(\d+)/);
  const workerPidMatch = result.stdout.match(/\[ts-unit trace\] launched index=0 worker=(\d+)/);

  assert.equal(result.code, 1, result.stdout + result.stderr);
  assert.match(result.stdout, /TS UNIT SUITE INCOMPLETE/);
  assert.match(result.stdout, /entrypoint deadline reached after 6s/);
  assert.match(result.stdout, /forwarding TERM to bash supervisor/);
  assert.match(result.stdout, /received TERM/);
  assert.match(result.stdout, /STILL RUNNING AT DEADLINE \(1\)/);
  assert.match(result.stdout, /✗ TS unit suite did not complete/);
  assert(bashPidMatch, result.stdout);
  assert(supervisorPidMatch, result.stdout);
  assert(workerPidMatch, result.stdout);
  assert(Number.isInteger(workerPid), result.stdout);
  assert.throws(() => process.kill(Number(bashPidMatch[1]), 0), { code: "ESRCH" });
  assert.throws(() => process.kill(workerPid, 0), { code: "ESRCH" });
});

test("portable per-file timeout keeps the timeout verdict and kills the test", async (t) => {
  const directory = await createFixtureDirectory(t);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  const connected = new Promise((resolve) => connectedResolve = resolve);
  const closed = new Promise((resolve) => closedResolve = resolve);
  server.once("connection", (socket) => {
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("close", closedResolve);
    connectedResolve();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  const timeoutFile = path.join(directory, "per-file-timeout.test.ts");
  await writeFile(timeoutFile, `
import net from "node:net";
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("error", () => {});
`);
  const listFile = path.join(directory, "tests.txt");
  await writeFile(listFile, `${timeoutFile}\n`);

  const run = runShellRunner({
    CODETERM_FORCE_PORTABLE_TIMEOUT: "1",
    RUNNER_TEST_PORT: String(server.address().port),
    TS_UNIT_JOBS: "1",
    TS_UNIT_TIMEOUT_SECONDS: "1",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "10",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
  });
  await connected;
  const result = await run;
  await closed;

  assert.equal(result.code, 1, result.stdout + result.stderr);
  assert.match(result.stdout, /✗ TIMED OUT after 1s:/);
  assert.match(result.stdout, /✗ 1 of 1 TS unit test file\(s\) failed/);
});
