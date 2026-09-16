import { existsSync, readFileSync, watch } from "node:fs";
import { dirname } from "node:path";
import { setTimeout as delay } from "node:timers";

const readinessSignal = (line, apiPort) =>
  line.includes("REST API generation ") &&
  line.includes(" health-proven on http://") &&
  new RegExp(`:${apiPort}(?:\\b|/)`).test(line);

const tail = (path) => {
  if (!existsSync(path)) return "";
  return readFileSync(path, "utf8").split(/\r?\n/).slice(-25).join("\n");
};

const exitResult = (path) => {
  if (!existsSync(path)) return undefined;
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return undefined;
  }
};

export function waitForReadiness({ log, exitFile, apiPort, deadlineMs = 600_000 }) {
  if (!log || !exitFile || !apiPort) throw new Error("dev wait log, exit file, and API port are required");
  return new Promise((resolve, reject) => {
    let settled = false;
    let partial = "";
    let logOffset = 0;
    let logWatcher;
    let exitWatcher;
    let deadline;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      logWatcher?.close();
      exitWatcher?.close();
      if (deadline) clearTimeout(deadline);
      if (error) reject(error);
      else resolve(value);
    };
    const inspectExit = () => {
      const result = exitResult(exitFile);
      if (result) {
        finish(new Error(`dev app exited before health readiness (code=${result.code ?? "unknown"}, signal=${result.signal ?? "none"})\n${tail(log)}`));
      }
    };
    const inspectLog = () => {
      const content = readFileSync(log, "utf8");
      const delta = content.slice(logOffset);
      logOffset = content.length;
      const lines = `${partial}${delta}`.split(/\r?\n/);
      partial = lines.pop() ?? "";
      const signal = lines.find((line) => readinessSignal(line, apiPort));
      if (signal) finish(null, { signal });
    };
    logWatcher = watch(log, { persistent: true }, inspectLog);
    exitWatcher = watch(dirname(exitFile), { persistent: true }, (_event, name) => {
      if (name === exitFile.split(/[\\/]/).pop()) inspectExit();
    });
    deadline = delay(() => {
      finish(new Error(`dev app did not report health readiness within ${deadlineMs}ms\n${tail(log)}`));
    }, deadlineMs);
    inspectLog();
    inspectExit();
  });
}

const option = (name) => {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
};

if (process.argv[1]?.endsWith("dev-wait-health.mjs")) {
  const log = option("--log");
  const exitFile = option("--exit-file");
  const apiPort = option("--api-port");
  if (!log || !exitFile || !apiPort) throw new Error("usage: dev-wait-health.mjs --log PATH --exit-file PATH --api-port PORT");
  waitForReadiness({ log, exitFile, apiPort }).then(({ signal }) => {
    console.log(`dev readiness signal: ${signal}`);
  }).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}

export { readinessSignal };
