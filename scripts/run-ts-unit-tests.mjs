import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

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
  signalSource = process,
  env = process.env,
  monitorDeadline = true,
  ...options
} = {}) {
  const bash = resolveBash({ ...options, env });
  if (!bash) {
    console.error("TS unit tests require Git Bash on Windows. Install Git for Windows or set CODETERM_BASH to bash.exe.");
    return Promise.resolve(1);
  }

  return new Promise((resolve, reject) => {
    const childEnv = { ...env, CODETERM_TS_UNIT_CONTROL_STDIN: "1" };
    const child = spawnProcess(bash, ["scripts/ts-unit-tests.sh"], {
      cwd: root,
      env: childEnv,
      stdio: ["pipe", "inherit", "inherit"],
    });
    const control = child.stdin;
    control?.on("error", () => {});
    if (env.TS_UNIT_TRACE) {
      console.log(`[ts-unit trace] entrypoint pid=${process.pid} parent=${process.ppid}`);
      console.log(`[ts-unit trace] bash process pid=${child.pid}`);
    }
    const forwardSignal = (signal) => {
      if (env.TS_UNIT_TRACE) {
        console.log(`[ts-unit trace] forwarding ${signal} to bash control pipe`);
      }
      if (control?.writable && !control.destroyed) {
        control.end(`${signal}\n`);
      } else {
        child.kill(signal);
      }
    };
    const forwardTerm = () => forwardSignal("TERM");
    const forwardInterrupt = () => forwardSignal("INT");
    let bashExited = false;
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
    child.once("exit", (code, signal) => {
      bashExited = true;
      removeSignalHandlers();
      control?.destroy();
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
