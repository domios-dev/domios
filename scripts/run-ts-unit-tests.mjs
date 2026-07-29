import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const supervisorBootstrap =
  'printf "__CODETERM_TS_UNIT_SUPERVISOR_PID__=%s\\n" "$$"; exec scripts/ts-unit-tests.sh "$@"';
const supervisorPidMarker = "__CODETERM_TS_UNIT_SUPERVISOR_PID__=";

export function resolveBash({ platform = process.platform, env = process.env, exists = existsSync } = {}) {
  if (platform !== "win32") return "bash";

  const candidates = [
    env.CODETERM_BASH,
    env.ProgramFiles && path.join(env.ProgramFiles, "Git", "bin", "bash.exe"),
    env["ProgramFiles(x86)"] && path.join(env["ProgramFiles(x86)"], "Git", "bin", "bash.exe"),
  ].filter(Boolean);

  return candidates.find(exists);
}

export function runTsUnitTests({
  spawnProcess = spawn,
  spawnSignalProcess = spawn,
  signalSource = process,
  output = process.stdout,
  env = process.env,
  platform = process.platform,
  monitorDeadline = true,
  filters = process.argv.slice(2),
  ...options
} = {}) {
  const bash = resolveBash({ ...options, env, platform });
  if (!bash) {
    console.error("TS unit tests require Git Bash on Windows. Install Git for Windows or set CODETERM_BASH to bash.exe.");
    return Promise.resolve(1);
  }

  return new Promise((resolve, reject) => {
    const child = spawnProcess(bash, ["-c", supervisorBootstrap, "codeterm-ts-unit", ...filters], {
      cwd: root,
      env,
      stdio: ["ignore", "pipe", "inherit"],
    });
    const supervisorPidStream = child.stdout;
    let supervisorPidBuffer = "";
    let supervisorPidRead = false;
    let supervisorPid;
    let pendingSignal;
    let bashExited = false;

    if (env.TS_UNIT_TRACE) {
      console.log(`[ts-unit trace] entrypoint pid=${process.pid} parent=${process.ppid}`);
      console.log(`[ts-unit trace] bash process pid=${child.pid}`);
    }

    const sendWindowsSignal = (signal) => {
      const killTree = () => {
        if (bashExited) return;
        const killer = spawnSignalProcess(
          "taskkill.exe",
          ["//PID", String(child.pid), "//T", "//F"],
          { stdio: "ignore" },
        );
        killer.once("error", () => child.kill(`SIG${signal}`));
      };
      const sender = spawnSignalProcess(
        bash,
        ["-c", `kill -${signal} -- ${supervisorPid}`],
        { cwd: root, env, stdio: "ignore" },
      );
      sender.once("error", killTree);
      sender.once("exit", (code) => {
        if (code !== 0) killTree();
      });
    };

    const sendSignal = (signal) => {
      if (platform === "win32") {
        if (!supervisorPid) {
          pendingSignal = signal;
          return;
        }
        sendWindowsSignal(signal);
        return;
      }
      child.kill(`SIG${signal}`);
    };

    supervisorPidStream?.setEncoding("utf8");
    supervisorPidStream?.on("data", (chunk) => {
      if (supervisorPidRead) {
        output.write(chunk);
        return;
      }
      supervisorPidBuffer += chunk;
      const newline = supervisorPidBuffer.indexOf("\n");
      if (newline < 0) return;

      const firstLine = supervisorPidBuffer.slice(0, newline).trim();
      const remainder = supervisorPidBuffer.slice(newline + 1);
      const parsedPid = firstLine.startsWith(supervisorPidMarker)
        ? Number(firstLine.slice(supervisorPidMarker.length))
        : Number.NaN;
      supervisorPidRead = true;
      supervisorPidBuffer = "";
      if (Number.isInteger(parsedPid) && parsedPid > 0) {
        supervisorPid = parsedPid;
        if (env.TS_UNIT_TRACE) {
          console.log(`[ts-unit trace] bash supervisor pid=${supervisorPid}`);
        }
        if (pendingSignal) {
          const signal = pendingSignal;
          pendingSignal = undefined;
          sendWindowsSignal(signal);
        }
      } else {
        output.write(`${firstLine}\n`);
      }
      if (remainder) output.write(remainder);
    });
    supervisorPidStream?.on("end", () => {
      if (!supervisorPidRead && supervisorPidBuffer) {
        output.write(supervisorPidBuffer);
      }
    });
    supervisorPidStream?.on("error", () => {});

    const forwardSignal = (signal) => {
      if (env.TS_UNIT_TRACE) {
        console.log(`[ts-unit trace] forwarding ${signal} to bash supervisor`);
      }
      sendSignal(signal);
    };
    const forwardTerm = () => forwardSignal("TERM");
    const forwardInterrupt = () => forwardSignal("INT");
    let deadlineSignal;
    let forwardDeadline;
    const suiteDeadlineSeconds = Number(
      env.CODETERM_TS_UNIT_ENTRYPOINT_TEST_DEADLINE_SECONDS ??
        env.TS_UNIT_SUITE_TIMEOUT_SECONDS ??
        80,
    );
    if (monitorDeadline && Number.isInteger(suiteDeadlineSeconds) && suiteDeadlineSeconds > 0) {
      deadlineSignal = AbortSignal.timeout(suiteDeadlineSeconds * 1000);
      forwardDeadline = () => {
        if (!bashExited) {
          if (env.TS_UNIT_TRACE) {
            console.log(`[ts-unit trace] entrypoint deadline reached after ${suiteDeadlineSeconds}s`);
          }
          forwardSignal("TERM");
        }
      };
      deadlineSignal.addEventListener("abort", forwardDeadline, { once: true });
    }
    const removeSignalHandlers = () => {
      signalSource.removeListener("SIGTERM", forwardTerm);
      signalSource.removeListener("SIGINT", forwardInterrupt);
      deadlineSignal?.removeEventListener("abort", forwardDeadline);
    };

    signalSource.once("SIGTERM", forwardTerm);
    signalSource.once("SIGINT", forwardInterrupt);
    child.once("error", (error) => {
      removeSignalHandlers();
      reject(error);
    });
    child.once("exit", () => {
      bashExited = true;
    });
    child.once("close", (code, signal) => {
      bashExited = true;
      removeSignalHandlers();
      if (env.TS_UNIT_TRACE) {
        console.log(`[ts-unit trace] bash exited code=${code} signal=${signal ?? "none"}`);
      }
      resolve(code ?? (signal ? 1 : 0));
    });
  });
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
