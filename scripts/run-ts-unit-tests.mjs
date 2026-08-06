import { spawn } from "node:child_process";
import {
  closeSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const testWorkerBootstrap = `
import { writeSync } from "node:fs";
import { pathToFileURL } from "node:url";

process.on("exit", () => {
  const usage = process.resourceUsage();
  try {
    writeSync(3, JSON.stringify({
      userCPUTime: usage.userCPUTime,
      systemCPUTime: usage.systemCPUTime,
    }) + "\\n");
  } catch {}
});

await import(pathToFileURL(process.argv[1]).href);
`;

function positiveInteger(value, name) {
  if (!/^[1-9][0-9]*$/.test(String(value))) {
    throw new Error(`${name} must be a positive integer`);
  }
  return Number(value);
}

function optionalPositiveInteger(value, name) {
  if (value === undefined || value === null || value === "") return undefined;
  return positiveInteger(value, name);
}

export function resolveTsUnitTimeouts(env) {
  return {
    testTimeoutSeconds: optionalPositiveInteger(
      env.TS_UNIT_TIMEOUT_SECONDS,
      "TS_UNIT_TIMEOUT_SECONDS",
    ),
    suiteTimeoutSeconds: optionalPositiveInteger(
      env.CODETERM_TS_UNIT_ENTRYPOINT_TEST_DEADLINE_SECONDS ||
        env.TS_UNIT_SUITE_TIMEOUT_SECONDS,
      "TS_UNIT_SUITE_TIMEOUT_SECONDS",
    ),
  };
}

function normalizeTestPath(file) {
  return file.split(path.sep).join("/");
}

function collectTests(directory, tests = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "dist") {
        collectTests(path.join(directory, entry.name), tests);
      }
      continue;
    }
    if (entry.isFile() && /\.test\.tsx?$/.test(entry.name)) {
      tests.push(normalizeTestPath(path.relative(root, path.join(directory, entry.name))));
    }
  }
  return tests;
}

function discoverTests(env) {
  if (env.CODETERM_TS_UNIT_RUNNER_TEST_LIST) {
    return readFileSync(env.CODETERM_TS_UNIT_RUNNER_TEST_LIST, "utf8")
      .split(/\r?\n/)
      .filter(Boolean);
  }
  return [
    ...collectTests(path.join(root, "src")),
    ...collectTests(path.join(root, "packages")),
  ].sort();
}

function filterTests(tests, filters) {
  if (filters.length === 0) return tests;
  return tests.filter((testFile) => filters.some((filter) => testFile.includes(filter)));
}

function createEventQueue() {
  const events = [];
  const waiters = [];
  return {
    push(event) {
      const waiter = waiters.shift();
      if (waiter) waiter(event);
      else events.push(event);
    },
    next() {
      if (events.length > 0) return Promise.resolve(events.shift());
      return new Promise((resolve) => waiters.push(resolve));
    },
  };
}

function onceClosed(child) {
  return new Promise((resolve) => {
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
}

function runTaskkill(pid, spawnProcess) {
  return new Promise((resolve) => {
    const killer = spawnProcess(
      "taskkill.exe",
      ["/PID", String(pid), "/T", "/F"],
      { stdio: "ignore", windowsHide: true },
    );
    killer.once("error", resolve);
    killer.once("close", resolve);
  });
}

async function terminateTree(child, signal, platform, spawnProcess) {
  if (!child.pid) return;
  if (platform === "win32") {
    await runTaskkill(child.pid, spawnProcess);
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") {
      try {
        child.kill(signal);
      } catch {}
    }
  }
}

function startWorker({
  index,
  testFile,
  testTimeoutSeconds,
  logDirectory,
  env,
  platform,
  spawnProcess,
  trace,
}) {
  const logFile = path.join(logDirectory, `${index}.log`);
  const logFd = openSync(logFile, "w");
  const absoluteTestPath = path.resolve(root, testFile);
  const started = process.hrtime.bigint();
  let child;
  try {
    child = spawnProcess(
      process.execPath,
      [
        "--import",
        "tsx",
        "--input-type=module",
        "--eval",
        testWorkerBootstrap,
        absoluteTestPath,
      ],
      {
        cwd: root,
        env,
        detached: platform !== "win32",
        stdio: ["ignore", logFd, logFd, "pipe"],
        windowsHide: true,
      },
    );
  } finally {
    closeSync(logFd);
  }

  trace(`started index=${index} child=${child.pid} file=${testFile}`);
  let metrics = "";
  let spawnError;
  let timedOut = false;
  let stopRequested = false;
  let closed = false;
  let forceSignal;

  child.stdio[3].setEncoding("utf8");
  child.stdio[3].on("data", (chunk) => {
    metrics += chunk;
  });
  child.once("error", (error) => {
    spawnError = error;
  });

  const closeResult = onceClosed(child);
  const requestStop = async (reason) => {
    if (closed || stopRequested) return;
    stopRequested = true;
    if (reason === "timeout") timedOut = true;
    await terminateTree(child, "SIGTERM", platform, spawnProcess);
    if (closed || platform === "win32") return;
    forceSignal = AbortSignal.timeout(5_000);
    forceSignal.addEventListener(
      "abort",
      () => void terminateTree(child, "SIGKILL", platform, spawnProcess),
      { once: true },
    );
  };

  const timeoutSignal = testTimeoutSeconds === undefined
    ? undefined
    : AbortSignal.timeout(testTimeoutSeconds * 1_000);
  timeoutSignal?.addEventListener("abort", () => void requestStop("timeout"), { once: true });

  const result = closeResult.then(({ code, signal }) => {
    closed = true;
    const elapsedMs = Number((process.hrtime.bigint() - started) / 1_000_000n);
    let cpuMs = 0;
    const metricLine = metrics.trim().split(/\r?\n/).at(-1);
    if (metricLine) {
      try {
        const usage = JSON.parse(metricLine);
        cpuMs = Math.round((usage.userCPUTime + usage.systemCPUTime) / 1_000);
      } catch {}
    }
    let status = code ?? (signal ? 1 : 0);
    if (spawnError) status = 127;
    if (timedOut) status = 124;
    else if (stopRequested) status = 143;
    return {
      index,
      testFile,
      logFile,
      status,
      elapsedMs,
      cpuMs,
      timedOut,
      stopped: stopRequested && !timedOut,
    };
  });

  return {
    child,
    result,
    stop: () => requestStop("suite"),
    timeoutSignal,
  };
}

function writeLine(stream, line = "") {
  stream.write(`${line}\n`);
}

function printTestResult(result, state, output, testTimeoutSeconds) {
  writeLine(output);
  writeLine(output, `════ ${result.testFile} (${result.elapsedMs}ms) ════`);
  output.write(readFileSync(result.logFile, "utf8"));
  if (result.stopped) {
    state.kind = "stopped";
    return;
  }
  if (result.status === 0) {
    state.kind = "passed";
    return;
  }
  state.kind = "failed";
  if (result.timedOut) {
    writeLine(output, `  ✗ TIMED OUT after ${testTimeoutSeconds}s: ${result.testFile}`);
  } else {
    writeLine(output, `  ✗ FAILED: ${result.testFile}`);
  }
}

function printTimingSummary({ results, jobs, suiteStarted, output }) {
  const totalWallMs = results.reduce((total, result) => total + result.elapsedMs, 0);
  const totalCpuMs = results.reduce((total, result) => total + result.cpuMs, 0);
  const serializedFloor = Math.ceil(totalWallMs / jobs);
  const observedSeconds = Math.floor(
    Number(process.hrtime.bigint() - suiteStarted) / 1_000_000_000,
  );
  writeLine(output);
  writeLine(output, `TS unit timing: ${results.length} completed file(s), TS_UNIT_JOBS=${jobs}`);
  writeLine(output, `Aggregate per-file wall time: ${totalWallMs}ms`);
  writeLine(output, `Total child CPU time: ${totalCpuMs}ms`);
  writeLine(
    output,
    `Width-${jobs} serialized floor: ceil(${totalWallMs}ms / ${jobs}) = ${serializedFloor}ms`,
  );
  writeLine(output, `Observed suite wall time: ${observedSeconds}s`);
  if (results.length === 0) return;
  writeLine(output, "Per-file wall time, slowest first:");
  for (const result of [...results].sort((left, right) => right.elapsedMs - left.elapsedMs)) {
    writeLine(
      output,
      `  ${String(result.elapsedMs).padStart(8)}ms wall  ` +
        `${String(result.cpuMs).padStart(8)}ms CPU  ${result.testFile}`,
    );
  }
}

function snapshotStates(states) {
  return {
    passed: states.filter((state) => state.kind === "passed"),
    failed: states.filter((state) => state.kind === "failed"),
    running: states.filter((state) => state.kind === "running"),
    pending: states.filter((state) => state.kind === "pending"),
  };
}

function printDeadlineSummary(snapshot, reason, elapsedSeconds, output) {
  writeLine(output);
  writeLine(output, "════════ TS UNIT SUITE INCOMPLETE ════════");
  writeLine(output, reason);
  writeLine(output, `PASSED AT DEADLINE (${snapshot.passed.length}):`);
  for (const state of snapshot.passed) {
    writeLine(output, `  ✓ ${state.testFile} (${state.elapsedMs}ms)`);
  }
  writeLine(output, `FAILED AT DEADLINE (${snapshot.failed.length}):`);
  for (const state of snapshot.failed) {
    writeLine(
      output,
      `  ✗ ${state.testFile} (exit ${state.status}, ${state.elapsedMs}ms)`,
    );
  }
  writeLine(output, `STILL RUNNING AT DEADLINE (${snapshot.running.length}):`);
  for (const state of snapshot.running) {
    writeLine(output, `  … ${state.testFile} (${elapsedSeconds}s suite elapsed)`);
  }
  writeLine(output, `NEVER STARTED (${snapshot.pending.length}):`);
  for (const state of snapshot.pending) {
    writeLine(output, `  - ${state.testFile}`);
  }
  writeLine(output, "✗ TS unit suite did not complete");
}

export async function runTsUnitTests({
  env = process.env,
  filters = process.argv.slice(2),
  output = process.stdout,
  errorOutput = process.stderr,
  signalSource = process,
  platform = process.platform,
  spawnProcess = spawn,
} = {}) {
  let jobs;
  let testTimeoutSeconds;
  let suiteTimeoutSeconds;
  try {
    jobs = positiveInteger(env.TS_UNIT_JOBS ?? 4, "TS_UNIT_JOBS");
    ({ testTimeoutSeconds, suiteTimeoutSeconds } = resolveTsUnitTimeouts(env));
  } catch (error) {
    writeLine(errorOutput, error.message);
    return 2;
  }

  const tests = filterTests(discoverTests(env), filters);
  if (tests.length === 0) {
    if (filters.length > 0) {
      writeLine(errorOutput, `no TS unit tests matched filter(s): ${filters.join(" ")}`);
      return 2;
    }
    writeLine(output, "no TS unit tests found under src/ or packages/");
    return 0;
  }

  const logDirectory = mkdtempSync(path.join(tmpdir(), "codeterm-ts-unit-"));
  const states = tests.map((testFile, index) => ({
    index,
    testFile,
    kind: "pending",
    status: undefined,
    elapsedMs: undefined,
  }));
  const events = createEventQueue();
  const running = new Map();
  const results = [];
  const suiteStarted = process.hrtime.bigint();
  let nextIndex = 0;
  let cancelled = false;
  const trace = (message) => {
    if (env.TS_UNIT_TRACE) writeLine(output, `[ts-unit trace] ${message}`);
  };
  const queueCancellation = (reason) => {
    if (cancelled) return;
    cancelled = true;
    events.push({ type: "cancel", reason });
  };
  const forwardTerm = () => queueCancellation("received TERM");
  const forwardInterrupt = () => queueCancellation("received INT");
  const deadlineSignal = suiteTimeoutSeconds === undefined
    ? undefined
    : AbortSignal.timeout(suiteTimeoutSeconds * 1_000);
  const onDeadline = () =>
    queueCancellation(`global deadline reached after ${suiteTimeoutSeconds}s`);
  deadlineSignal?.addEventListener("abort", onDeadline, { once: true });
  signalSource.once("SIGTERM", forwardTerm);
  signalSource.once("SIGINT", forwardInterrupt);

  const launch = (index) => {
    const state = states[index];
    state.kind = "running";
    const worker = startWorker({
      index,
      testFile: state.testFile,
      testTimeoutSeconds,
      logDirectory,
      env,
      platform,
      spawnProcess,
      trace,
    });
    running.set(index, worker);
    trace(`launched index=${index} active=${running.size} file=${state.testFile}`);
    worker.result.then((result) => events.push({ type: "done", result }));
  };

  const finish = (result) => {
    const state = states[result.index];
    running.delete(result.index);
    state.status = result.status;
    state.elapsedMs = result.elapsedMs;
    results.push(result);
    printTestResult(result, state, output, testTimeoutSeconds);
    trace(
      `completed index=${result.index} status=${result.status} ` +
        `active=${running.size} file=${result.testFile}`,
    );
  };

  try {
    while (nextIndex < tests.length || running.size > 0) {
      while (!cancelled && running.size < jobs && nextIndex < tests.length) {
        launch(nextIndex);
        nextIndex += 1;
      }
      const event = await events.next();
      if (event.type === "done") {
        finish(event.result);
        continue;
      }

      const elapsedSeconds = Math.floor(
        Number(process.hrtime.bigint() - suiteStarted) / 1_000_000_000,
      );
      const snapshot = snapshotStates(states);
      writeLine(
        errorOutput,
        `TS unit runner: ${event.reason}; terminating ${snapshot.running.length} running worker(s)`,
      );
      const activeWorkers = [...running.values()];
      await Promise.all(activeWorkers.map((worker) => worker.stop()));
      const cleanupResults = await Promise.all(activeWorkers.map((worker) => worker.result));
      for (const result of cleanupResults) {
        if (running.has(result.index)) finish(result);
      }
      printTimingSummary({ results, jobs, suiteStarted, output });
      printDeadlineSummary(snapshot, event.reason, elapsedSeconds, output);
      return 1;
    }

    printTimingSummary({ results, jobs, suiteStarted, output });
    const failures = states.filter((state) => state.kind === "failed").length;
    writeLine(output);
    if (failures === 0) {
      writeLine(output, `✓ all ${tests.length} TS unit test file(s) green`);
      return 0;
    }
    writeLine(output, `✗ ${failures} of ${tests.length} TS unit test file(s) failed`);
    return 1;
  } finally {
    signalSource.removeListener("SIGTERM", forwardTerm);
    signalSource.removeListener("SIGINT", forwardInterrupt);
    deadlineSignal?.removeEventListener("abort", onDeadline);
    rmSync(logDirectory, { recursive: true, force: true });
    trace("cleanup complete");
  }
}

export async function runTsUnitTestMain({
  runTests = runTsUnitTests,
  exitProcess = process.exit,
  reportError = (error) => console.error(error.message),
} = {}) {
  try {
    exitProcess(await runTests());
  } catch (error) {
    reportError(error);
    exitProcess(1);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  void runTsUnitTestMain();
}
