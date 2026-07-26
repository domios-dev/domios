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

export function runTsUnitTests({ spawnProcess = spawn, ...options } = {}) {
  const bash = resolveBash(options);
  if (!bash) {
    console.error("TS unit tests require Git Bash on Windows. Install Git for Windows or set CODETERM_BASH to bash.exe.");
    return Promise.resolve(1);
  }

  return new Promise((resolve, reject) => {
    const child = spawnProcess(bash, ["scripts/ts-unit-tests.sh"], {
      cwd: root,
      env: process.env,
      stdio: "inherit",
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve(code ?? (signal ? 1 : 0)));
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runTsUnitTests().then((code) => process.exitCode = code).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
