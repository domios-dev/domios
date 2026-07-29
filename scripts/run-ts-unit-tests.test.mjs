import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter, once } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { resolveBash, runTsUnitTests } from "./run-ts-unit-tests.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function runShellRunner(env) {
  const bash = resolveBash();
  return new Promise((resolve, reject) => {
    const child = spawn(bash, ["scripts/ts-unit-tests.sh"], {
      cwd: root,
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => stdout += chunk);
    child.stderr.setEncoding("utf8").on("data", (chunk) => stderr += chunk);
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code: code ?? 1, signal, stdout, stderr }));
  });
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
  child.kill = () => {};
  const signalSource = new EventEmitter();
  const env = { ProgramFiles: "C:\\Program Files" };
  const code = runTsUnitTests({
    platform: "win32",
    env,
    signalSource,
    exists: (file) => file === bash,
    spawnProcess(command, args, options) {
      assert.equal(command, bash);
      assert.deepEqual(args, ["scripts/ts-unit-tests.sh"]);
      assert.equal(options.stdio, "inherit");
      assert.equal(options.env, env);
      queueMicrotask(() => child.emit("exit", 0, null));
      return child;
    },
  });
  assert.equal(await code, 0);
});

test("forwards termination to the shell runner", async () => {
  const child = new EventEmitter();
  const signalSource = new EventEmitter();
  let forwardedSignal;
  child.kill = (signal) => {
    forwardedSignal = signal;
    queueMicrotask(() => child.emit("exit", 1, null));
  };

  const result = runTsUnitTests({
    platform: "linux",
    signalSource,
    spawnProcess: () => child,
  });
  signalSource.emit("SIGTERM");

  assert.equal(await result, 1);
  assert.equal(forwardedSignal, "SIGTERM");
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

async function verifySuiteDeadline(t, forcePortable) {
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

  const run = runShellRunner({
    CODETERM_FORCE_PORTABLE_TIMEOUT: forcePortable ? "1" : "",
    RUNNER_TEST_PORT: String(server.address().port),
    TS_UNIT_JOBS: "1",
    TS_UNIT_TIMEOUT_SECONDS: "20",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "1",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
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
