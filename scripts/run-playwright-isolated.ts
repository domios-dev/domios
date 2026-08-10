import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { assertReusableDevConsole } from "../tests/playwright/helpers/dev-console";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TEST_ROOT = path.join(REPO_ROOT, "tests", "playwright");
const PLAYWRIGHT_CLI = path.join(REPO_ROOT, "node_modules", "@playwright", "test", "cli.js");
const TEST_TIMEOUT_MS = 30_000;
const GLOBAL_TIMEOUT_MS = 600_000;

export type IsolatedPlaywrightArgs = {
  file: string;
  grep: string;
  project: string;
};

export function parsePlaywrightSpec(fileArgument: string): string {
  const file = path.resolve(REPO_ROOT, fileArgument);
  const relative = path.relative(TEST_ROOT, file);
  if (relative.startsWith("..") || path.isAbsolute(relative) || !/\.spec\.tsx?$/.test(file)) {
    throw new Error("--file must name a Playwright .spec.ts or .spec.tsx file under tests/playwright");
  }
  if (!fs.statSync(file, { throwIfNoEntry: false })?.isFile()) {
    throw new Error(`Playwright spec does not exist: ${fileArgument}`);
  }
  // Playwright reads the positional filter as a REGEX. A Windows path turns
  // `\suites` into a whitespace class, so it can never match its own file.
  return path.relative(REPO_ROOT, file).split(path.sep).join("/");
}

function requiredOption(args: string[], name: string): string {
  const values = args.flatMap((arg, index) => (arg === name ? [args[index + 1]] : []));
  if (values.length !== 1 || !values[0]?.trim()) {
    throw new Error(`${name} must be supplied exactly once with a non-empty value`);
  }
  return values[0];
}

/** `pnpm run <script> -- --file X` forwards the separator verbatim; drop it. */
export function stripArgumentSeparator(args: string[]): string[] {
  return args[0] === "--" ? args.slice(1) : args;
}

export function parseIsolatedPlaywrightArgs(rawArgs: string[]): IsolatedPlaywrightArgs {
  const args = stripArgumentSeparator(rawArgs);
  const fileArgument = requiredOption(args, "--file");
  const project = requiredOption(args, "--project");
  const grep = requiredOption(args, "--grep");
  const allowed = new Set(["--file", "--project", "--grep"]);
  for (let index = 0; index < args.length; index += 1) {
    if (!allowed.has(args[index])) throw new Error(`unsupported option ${args[index]}`);
    index += 1;
  }

  const file = parsePlaywrightSpec(fileArgument);
  return { file, grep, project };
}

export function playwrightArgs(input: IsolatedPlaywrightArgs): string[] {
  return [
    PLAYWRIGHT_CLI,
    "test",
    "--config=tests/playwright/playwright.config.ts",
    `--project=${input.project}`,
    input.file,
    "-g",
    input.grep,
    `--timeout=${TEST_TIMEOUT_MS}`,
    `--global-timeout=${GLOBAL_TIMEOUT_MS}`,
    "--reporter=line",
  ];
}

export function runStreaming(
  command: string,
  args: string[],
  env: NodeJS.ProcessEnv,
  deadlineMs = GLOBAL_TIMEOUT_MS,
  label = "isolated",
): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: REPO_ROOT, env, stdio: ["ignore", "pipe", "pipe"] });
    let deadlineExceeded = false;
    let settled = false;
    child.stdout.on("data", (chunk: Buffer) => process.stdout.write(chunk));
    child.stderr.on("data", (chunk: Buffer) => process.stderr.write(chunk));
    const deadline = setTimeout(() => {
      deadlineExceeded = true;
      process.stderr.write(`[playwright:${label}] process deadline ${deadlineMs}ms exceeded; terminating test process\n`);
      child.kill();
      settled = true;
      reject(new Error(`${label} Playwright run exceeded ${deadlineMs}ms`));
    }, deadlineMs);
    const finish = () => clearTimeout(deadline);
    child.once("error", (error) => {
      if (settled) return;
      settled = true;
      finish();
      reject(error);
    });
    child.once("close", (code, signal) => {
      if (settled) return;
      settled = true;
      finish();
      if (deadlineExceeded) {
        reject(new Error(`${label} Playwright run exceeded ${deadlineMs}ms`));
      } else if (code !== 0) {
        reject(new Error(`${label} Playwright exited with ${signal ?? `code ${code}`}`));
      } else {
        resolve();
      }
    });
  });
}

export async function main(args: string[]): Promise<void> {
  const input = parseIsolatedPlaywrightArgs(args);
  await assertReusableDevConsole();
  process.stdout.write(`[playwright:isolated] adopted ${process.env.CT_E2E_INSTANCE ?? "playwright"}; running ${path.relative(REPO_ROOT, input.file)}\n`);
  await runStreaming(process.execPath, playwrightArgs(input), {
    ...process.env,
    CT_E2E_REUSE_ONLY: "1",
    CT_E2E_WORKERS: "1",
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error: unknown) => {
    process.stderr.write(`[playwright:isolated] ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
