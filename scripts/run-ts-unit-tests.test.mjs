import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter, once } from "node:events";
import { existsSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { runTsUnitTestMain, runTsUnitTests } from "./run-ts-unit-tests.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const gitBinBash = path.join(
  process.env.ProgramFiles ?? "C:\\Program Files",
  "Git",
  "bin",
  "bash.exe",
);
const gitUsrBash = path.join(
  process.env.ProgramFiles ?? "C:\\Program Files",
  "Git",
  "usr",
  "bin",
  "bash.exe",
);
const defaultBash = process.platform === "win32" ? gitBinBash : "bash";

function runProcess(command, args, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: root,
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
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
    child.once("close", (code, signal) => {
      resolve({ code: code ?? 1, signal, stdout, stderr });
    });
  });
}

function runNodeEntrypoint(env, filters = []) {
  return runProcess(process.execPath, ["scripts/run-ts-unit-tests.mjs", ...filters], env);
}

function runShellEntrypoint(bash, env, filters = []) {
  return runProcess(bash, ["scripts/ts-unit-tests.sh", ...filters], env);
}

function runNpmEntrypoint(env) {
  return runProcess(defaultBash, ["-lc", "npm run test:unit"], env);
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

async function writeTestList(t, files) {
  const directory = await createFixtureDirectory(t);
  const listFile = path.join(directory, "tests.txt");
  const listedFiles = files.map((file) =>
    path.isAbsolute(file) || file.includes("/") ? file : path.join(directory, file),
  );
  await writeFile(listFile, `${listedFiles.join("\n")}\n`);
  return { directory, listFile };
}

test("rejects invalid scheduler widths", async () => {
  const stderr = new PassThrough();
  let text = "";
  stderr.setEncoding("utf8").on("data", (chunk) => {
    text += chunk;
  });
  const code = await runTsUnitTests({
    env: { ...process.env, TS_UNIT_JOBS: "0" },
    errorOutput: stderr,
    filters: [],
  });
  assert.equal(code, 2);
  assert.match(text, /TS_UNIT_JOBS must be a positive integer/);
});

test("entrypoint exits with the scheduler verdict", async () => {
  const exitCodes = [];
  await runTsUnitTestMain({
    runTests: async () => 1,
    exitProcess: (code) => exitCodes.push(code),
  });
  assert.deepEqual(exitCodes, [1]);
});

test("reaps a completed worker before an older blocked worker", async (t) => {
  const { directory, listFile } = await writeTestList(t, [
    "slow.test.ts",
    "fast.test.ts",
    "third.test.ts",
  ]);
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
  if (name === "slow") await once(socket, "data");
  socket.end();
  await once(socket, "close");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
`;
  await Promise.all(
    ["slow.test.ts", "fast.test.ts", "third.test.ts"].map((file) =>
      writeFile(path.join(directory, file), fixtureSource),
    ),
  );

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

  const result = await runNodeEntrypoint({
    RUNNER_TEST_PORT: String(server.address().port),
    TS_UNIT_JOBS: "2",
    TS_UNIT_TIMEOUT_SECONDS: "10",
    TS_UNIT_SUITE_TIMEOUT_SECONDS: "20",
    CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
    TS_UNIT_TRACE: "1",
  });

  assert.equal(result.code, 0, result.stdout + result.stderr);
  assert.deepEqual([...connections].sort(), ["fast", "slow", "third"]);
  const fastCompleted = result.stdout.indexOf("[ts-unit trace] completed index=1");
  const thirdLaunched = result.stdout.indexOf("[ts-unit trace] launched index=2");
  const slowCompleted = result.stdout.indexOf("[ts-unit trace] completed index=0");
  assert(fastCompleted >= 0, result.stdout);
  assert(thirdLaunched > fastCompleted, result.stdout);
  assert(slowCompleted > thirdLaunched, result.stdout);
});

test("real width-20 topology completes one hundred Canvas files", async () => {
  const result = await runNodeEntrypoint(
    {
      TS_UNIT_JOBS: "20",
      TS_UNIT_TIMEOUT_SECONDS: "60",
      TS_UNIT_SUITE_TIMEOUT_SECONDS: "30",
      TS_UNIT_TRACE: "1",
    },
    ["src/components/Canvas"],
  );
  assert.equal(result.code, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /TS unit timing: 100 completed file\(s\), TS_UNIT_JOBS=20/);
  assert.match(result.stdout, /✓ all 100 TS unit test file\(s\) green/);
  assert.equal(
    [...result.stdout.matchAll(/\[ts-unit trace\] completed index=/g)].length,
    100,
  );
});

test("both Bash wrappers ignore the removed control-fd switch", async (t) => {
  const files = [
    "packages/chat-core/src/dedupe.test.ts",
    "packages/chat-core/src/markers.test.ts",
    "packages/chat-core/src/merge.test.ts",
    "packages/chat-core/src/sessionReset.test.ts",
  ];
  const { listFile } = await writeTestList(t, files);
  const bashes = process.platform === "win32"
    ? [gitUsrBash, gitBinBash].filter(existsSync)
    : [defaultBash];
  for (const bash of bashes) {
    for (const control of ["", "1"]) {
      const result = await runShellEntrypoint(bash, {
        TS_UNIT_JOBS: "4",
        TS_UNIT_TIMEOUT_SECONDS: "60",
        TS_UNIT_SUITE_TIMEOUT_SECONDS: "10",
        CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
        CODETERM_TS_UNIT_CONTROL_STDIN: control,
      });
      assert.equal(result.code, 0, `${bash} control=${control}\n${result.stdout}${result.stderr}`);
      assert.match(result.stdout, /✓ all 4 TS unit test file\(s\) green/);
    }
  }
});

async function verifySuiteDeadline(t, forcePortable) {
  const { directory, listFile } = await writeTestList(t, [
    "deadline.test.ts",
    "never-started.test.ts",
  ]);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  const connected = new Promise((resolve) => {
    connectedResolve = resolve;
  });
  const closed = new Promise((resolve) => {
    closedResolve = resolve;
  });
  server.once("connection", (socket) => {
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("close", closedResolve);
    connectedResolve();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  await writeFile(path.join(directory, "deadline.test.ts"), `
import net from "node:net";
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("connect", () => socket.write("connected\\n"));
socket.on("error", () => {});
`);
  await writeFile(
    path.join(directory, "never-started.test.ts"),
    `throw new Error("must not start");\n`,
  );

  const run = runNodeEntrypoint({
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
  assert.match(result.stdout, /NEVER STARTED \(1\)/);
  assert.match(result.stdout, /✗ TS unit suite did not complete/);
  const workerPid = Number(
    result.stdout.match(/\[ts-unit trace\] started index=0 child=(\d+)/)?.[1],
  );
  assert(Number.isInteger(workerPid), result.stdout);
  assert.throws(() => process.kill(workerPid, 0), { code: "ESRCH" });
}

test("suite deadline reports every state and kills descendants", (t) =>
  verifySuiteDeadline(t, false));

test("portable-mode smoke uses the same bounded scheduler", (t) =>
  verifySuiteDeadline(t, true));

test("real npm topology prints its deadline verdict and leaves no worker", async (t) => {
  const { directory, listFile } = await writeTestList(t, ["npm-deadline.test.ts"]);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  const connected = new Promise((resolve) => {
    connectedResolve = resolve;
  });
  const closed = new Promise((resolve) => {
    closedResolve = resolve;
  });
  server.once("connection", (socket) => {
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("close", closedResolve);
    connectedResolve();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  await writeFile(path.join(directory, "npm-deadline.test.ts"), `
import net from "node:net";
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("connect", () => socket.write("connected\\n"));
socket.on("error", () => {});
`);

  const run = runNpmEntrypoint({
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
  assert.match(result.stdout, /global deadline reached after 1s/);
  const workerPid = Number(
    result.stdout.match(/\[ts-unit trace\] started index=0 child=(\d+)/)?.[1],
  );
  assert.throws(() => process.kill(workerPid, 0), { code: "ESRCH" });
});

test("per-file timeout preserves its verdict and kills the test", async (t) => {
  const { directory, listFile } = await writeTestList(t, ["timeout.test.ts"]);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  const connected = new Promise((resolve) => {
    connectedResolve = resolve;
  });
  const closed = new Promise((resolve) => {
    closedResolve = resolve;
  });
  server.once("connection", (socket) => {
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("close", closedResolve);
    connectedResolve();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  await writeFile(path.join(directory, "timeout.test.ts"), `
import net from "node:net";
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("connect", () => socket.write("connected\\n"));
socket.on("error", () => {});
`);

  const run = runNodeEntrypoint({
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

test("signal event produces a partial summary and cleans the worker", async (t) => {
  const { directory, listFile } = await writeTestList(t, ["signal.test.ts"]);
  const server = net.createServer();
  t.after(() => closeServer(server));
  let connectedResolve;
  let closedResolve;
  const connected = new Promise((resolve) => {
    connectedResolve = resolve;
  });
  const closed = new Promise((resolve) => {
    closedResolve = resolve;
  });
  server.once("connection", (socket) => {
    socket.on("error", (error) => {
      if (error.code !== "ECONNRESET") throw error;
    });
    socket.once("close", closedResolve);
    connectedResolve();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  await writeFile(path.join(directory, "signal.test.ts"), `
import net from "node:net";
const socket = net.createConnection({
  host: "127.0.0.1",
  port: Number(process.env.RUNNER_TEST_PORT),
});
socket.on("connect", () => socket.write("connected\\n"));
socket.on("error", () => {});
`);

  const signalSource = new EventEmitter();
  const stdout = new PassThrough();
  const stderr = new PassThrough();
  let output = "";
  stdout.setEncoding("utf8").on("data", (chunk) => {
    output += chunk;
  });
  const run = runTsUnitTests({
    env: {
      ...process.env,
      RUNNER_TEST_PORT: String(server.address().port),
      TS_UNIT_JOBS: "1",
      TS_UNIT_TIMEOUT_SECONDS: "20",
      TS_UNIT_SUITE_TIMEOUT_SECONDS: "20",
      CODETERM_TS_UNIT_RUNNER_TEST_LIST: listFile,
      TS_UNIT_TRACE: "1",
    },
    output: stdout,
    errorOutput: stderr,
    signalSource,
  });
  await connected;
  signalSource.emit("SIGTERM");
  const code = await run;
  await closed;
  assert.equal(code, 1, output);
  assert.match(output, /received TERM/);
  assert.match(output, /TS UNIT SUITE INCOMPLETE/);
  const workerPid = Number(
    output.match(/\[ts-unit trace\] started index=0 child=(\d+)/)?.[1],
  );
  assert.throws(() => process.kill(workerPid, 0), { code: "ESRCH" });
});
