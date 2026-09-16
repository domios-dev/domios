import { closeSync, existsSync, fstatSync, fsyncSync, openSync, readFileSync } from "node:fs";
import { watch } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const readinessSignal = (line, apiPort) =>
  line.includes("REST API generation ") &&
  line.includes(" health-proven on http://") &&
  new RegExp(`:${apiPort}(?:\\b|/)`).test(line);

const tail = (path) => {
  if (!existsSync(path)) return "";
  return readFileSync(path, "utf8").split(/\r?\n/).slice(-25).join("\n");
};

export function resolveLaunchCommand(command, env = process.env) {
  if (process.platform !== "win32" || command[0] !== "pnpm") return command;
  const result = spawnSync("where.exe", ["pnpm.cmd"], { env, encoding: "utf8" });
  const commandPath = result.status === 0 ? result.stdout.split(/\r?\n/).find(Boolean) : undefined;
  if (!commandPath) throw new Error("could not locate pnpm.cmd on PATH");
  const resolvedCommandPath = commandPath.trim();
  const batch = readFileSync(resolvedCommandPath, "utf8");
  const match = batch.match(/((?:%~dp0[\\/])?[^\s"]*pnpm\.cjs)/i);
  const relativeScript = match?.[1]?.replace(/^%~dp0[\\/]/i, "") ?? "pnpm.cjs";
  const scriptPath = resolve(dirname(resolvedCommandPath), relativeScript);
  if (!existsSync(scriptPath)) throw new Error(`pnpm.cjs is absent for ${resolvedCommandPath}`);
  return [process.execPath, scriptPath, ...command.slice(1)];
}

export function waitForReadiness({ log, apiPort, command, cwd = process.cwd(), env = process.env, deadlineMs = 600_000 }) {
  if (!Array.isArray(command) || command.length === 0) throw new Error("dev wait command is required");
  const launch = resolveLaunchCommand(command, env);
  const logFd = openSync(log, "a");
  const child = spawn(launch[0], launch.slice(1), {
    cwd,
    env,
    detached: true,
    shell: false,
    stdio: ["ignore", logFd, logFd],
    windowsHide: true,
  });

  return new Promise((resolve, reject) => {
    let offset = fstatSync(logFd).size;
    let partial = "";
    let settled = false;
    let childExited = false;
    let watcher;
    let deadline;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      watcher?.close();
      if (deadline) clearTimeout(deadline);
      closeSync(logFd);
      if (error) {
        if (!childExited && child.pid) {
          if (process.platform === "win32") spawnSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
          else {
            try { process.kill(-child.pid, "SIGTERM"); } catch { child.kill("SIGTERM"); }
          }
        }
        reject(error);
      }
      else {
        child.unref();
        resolve(value);
      }
    };
    const inspect = () => {
      const content = readFileSync(log, "utf8");
      const delta = content.slice(offset);
      offset = content.length;
      const lines = `${partial}${delta}`.split(/\r?\n/);
      partial = lines.pop() ?? "";
      const signal = lines.find((line) => readinessSignal(line, apiPort));
      if (signal) finish(null, { pid: child.pid, signal });
    };
    watcher = watch(log, { persistent: true }, inspect);
    deadline = delay(() => {
      finish(new Error(`dev app did not report health readiness within ${deadlineMs}ms\n${tail(log)}`));
    }, deadlineMs);
    child.once("error", (error) => finish(new Error(`dev app could not start: ${error.message}`)));
    child.once("exit", (code, signal) => {
      childExited = true;
      inspect();
      if (!settled) finish(new Error(`dev app exited before health readiness (code=${code}, signal=${signal})\n${tail(log)}`));
    });
    inspect();
    fsyncSync(logFd);
  });
}

const option = (name) => {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
};

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  const separator = process.argv.indexOf("--");
  const command = separator < 0 ? [] : process.argv.slice(separator + 1);
  const log = option("--log");
  const apiPort = option("--api-port");
  if (!log || !apiPort || command.length === 0) throw new Error("usage: dev-wait-health.mjs --log PATH --api-port PORT -- COMMAND [ARGS...]");
  waitForReadiness({ log, apiPort, command }).then(({ pid, signal }) => {
    console.error(`dev readiness signal: ${signal}`);
    process.stdout.write(String(pid));
  }).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}

export { readinessSignal };
