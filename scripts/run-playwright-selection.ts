/**
 * Adopt-only multi-spec runner.
 *
 * `test:pw:one` reuses a standing instance but accepts exactly one file;
 * `test:pw:bounded` accepts many files but kills, stands up and tears down.
 * This runner is the missing pair: many files, `CT_E2E_REUSE_ONLY=1`, no
 * lifecycle change at all.
 *
 * The file list is DERIVED from Playwright's own discovery of `--project`
 * plus `--grep`, never supplied and never hardcoded: a spec missing from a
 * hand-written list reads as covered when it never ran. The discovered set is
 * compared against what the scoped run reported, so scoping cannot drop a spec
 * silently — a zero-match filter exits 0, and an exit code is not an oracle.
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { assertReusableDevConsole } from "../tests/playwright/helpers/dev-console";
import { runStreaming, stripArgumentSeparator } from "./run-playwright-isolated";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PLAYWRIGHT_CLI = path.join(REPO_ROOT, "node_modules", "@playwright", "test", "cli.js");
const CONFIG = "tests/playwright/playwright.config.ts";
const ARTIFACT_DIR = path.join(REPO_ROOT, "tests", "playwright", "artifacts", "selection");
const DISCOVERY_TIMEOUT_MS = 300_000;
const RUN_TIMEOUT_MS = 900_000;
const REUSE_ENV = { CT_E2E_REUSE_ONLY: "1", CT_E2E_WORKERS: "1" } as const;

export interface SelectionArgs {
  project: string;
  grep: string;
  timeoutMs?: number;
}

/** `file :: describe > test` — stable across the list pass and the run pass. */
export interface DiscoveredTest {
  file: string;
  key: string;
}

interface JsonSpec {
  title?: string;
  file?: string;
}

interface JsonSuite {
  title?: string;
  file?: string;
  specs?: JsonSpec[];
  suites?: JsonSuite[];
}

export interface JsonReport {
  config?: { rootDir?: string };
  suites?: JsonSuite[];
}

export function parseSelectionArgs(rawArgs: string[]): SelectionArgs {
  const args = stripArgumentSeparator(rawArgs);
  const allowed = new Set(["--project", "--grep", "--timeout"]);
  if (args.length === 0 || args.length % 2 !== 0) {
    throw new Error("options must be supplied as name/value pairs");
  }
  const values = new Map<string, string[]>();
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    if (!allowed.has(name)) throw new Error(`unsupported option ${name}`);
    values.set(name, [...(values.get(name) ?? []), args[index + 1]]);
  }
  const single = (name: string, required: boolean): string | undefined => {
    const found = values.get(name) ?? [];
    if (found.length > 1 || (required && found.length !== 1) || found.some((value) => !value?.trim())) {
      throw new Error(`${name} must be supplied ${required ? "exactly once" : "at most once"} with a non-empty value`);
    }
    return found[0];
  };
  const project = single("--project", true)!;
  const grep = single("--grep", true)!;
  const timeout = single("--timeout", false);
  if (timeout !== undefined && !/^[1-9][0-9]*$/.test(timeout)) {
    throw new Error("--timeout must be a positive integer of milliseconds");
  }
  return { project, grep, timeoutMs: timeout === undefined ? undefined : Number(timeout) };
}

function toPosix(value: string): string {
  return value.split("\\").join("/");
}

/**
 * The JSON reporter nests describes under a file-level suite whose title IS the
 * file path, so depth 0 contributes no title. Only file suites carry `file`
 * reliably, so it is threaded down rather than re-derived per node.
 */
export function flattenReport(report: JsonReport): DiscoveredTest[] {
  const out: DiscoveredTest[] = [];
  const walk = (suite: JsonSuite, inheritedFile: string | undefined, titles: string[], depth: number): void => {
    const file = suite.file ?? inheritedFile;
    const trail = depth === 0 ? titles : [...titles, suite.title ?? ""];
    for (const spec of suite.specs ?? []) {
      const specFile = spec.file ?? file;
      if (!specFile) throw new Error(`Playwright report has a spec with no file: ${spec.title ?? "<untitled>"}`);
      const posix = toPosix(specFile);
      out.push({ file: posix, key: `${posix} :: ${[...trail.filter(Boolean), spec.title ?? ""].join(" > ")}` });
    }
    for (const child of suite.suites ?? []) walk(child, file, trail, depth + 1);
  };
  for (const suite of report.suites ?? []) walk(suite, undefined, [], 0);
  return out;
}

/**
 * Report paths are relative to the config rootDir, while the CLI's positional
 * filter is a regex resolved against the repo root — a native separator would
 * turn `\suites` into a whitespace class and match nothing.
 */
export function repoRelativeSpecPaths(report: JsonReport, tests: DiscoveredTest[], repoRoot = REPO_ROOT): string[] {
  const rootDir = report.config?.rootDir ?? repoRoot;
  return [...new Set(tests.map((test) => test.file))].sort().map((file) => {
    const absolute = path.isAbsolute(file) ? file : path.resolve(rootDir, file);
    const relative = path.relative(repoRoot, absolute);
    if (relative.startsWith("..")) throw new Error(`discovered spec escapes the repository: ${file}`);
    return toPosix(relative);
  });
}

export interface SelectionVerdict {
  discovered: number;
  reported: number;
  missing: string[];
  unexpected: string[];
}

/**
 * The count oracle. An exit code cannot separate "every selected test ran" from
 * "the file scoping quietly excluded a spec"; set equality can, and it names the
 * spec instead of reporting a number that is merely short.
 */
export function compareSelection(discovered: DiscoveredTest[], reported: DiscoveredTest[]): SelectionVerdict {
  const discoveredKeys = new Set(discovered.map((test) => test.key));
  const reportedKeys = new Set(reported.map((test) => test.key));
  return {
    discovered: discoveredKeys.size,
    reported: reportedKeys.size,
    missing: [...discoveredKeys].filter((key) => !reportedKeys.has(key)).sort(),
    unexpected: [...reportedKeys].filter((key) => !discoveredKeys.has(key)).sort(),
  };
}

export function selectionFailure(verdict: SelectionVerdict): Error | null {
  if (verdict.discovered === 0) {
    return new Error("the selection matched no tests; a zero-match filter exits 0, so this is a failure, not a pass");
  }
  if (verdict.missing.length === 0 && verdict.unexpected.length === 0) return null;
  return new Error(
    [
      `selection ran ${verdict.reported} of ${verdict.discovered} discovered tests`,
      ...verdict.missing.map((key) => `  NEVER RAN: ${key}`),
      ...verdict.unexpected.map((key) => `  RAN BUT WAS NOT DISCOVERED: ${key}`),
    ].join("\n"),
  );
}

export function discoveryArgs(input: SelectionArgs): string[] {
  return [
    PLAYWRIGHT_CLI,
    "test",
    `--config=${CONFIG}`,
    `--project=${input.project}`,
    "-g",
    input.grep,
    "--list",
    "--reporter=json",
  ];
}

export function runArgs(input: SelectionArgs, files: string[]): string[] {
  return [
    PLAYWRIGHT_CLI,
    "test",
    `--config=${CONFIG}`,
    `--project=${input.project}`,
    ...files,
    "-g",
    input.grep,
    "--workers=1",
    ...(input.timeoutMs === undefined ? [] : [`--timeout=${input.timeoutMs}`]),
    `--global-timeout=${RUN_TIMEOUT_MS}`,
    "--reporter=line,json",
  ];
}

/**
 * Discovery's exit code is not the answer — Playwright exits nonzero on "no
 * tests found", which is a selection verdict this runner reports itself, with
 * the selection in the message. Only a dead process is an error here.
 */
function captureStreaming(args: string[], env: NodeJS.ProcessEnv, deadlineMs: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, { cwd: REPO_ROOT, env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let settled = false;
    child.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer) => process.stderr.write(chunk));
    const deadline = setTimeout(() => {
      settled = true;
      child.kill();
      reject(new Error(`Playwright discovery exceeded ${deadlineMs}ms`));
    }, deadlineMs);
    child.once("error", (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      reject(error);
    });
    child.once("close", () => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      resolve(stdout);
    });
  });
}

/** The JSON reporter writes to PLAYWRIGHT_JSON_OUTPUT_NAME when set and to stdout otherwise. */
export function parseJsonReport(fileText: string | null, stdout: string, label: string): JsonReport {
  const start = stdout.indexOf("{");
  const source = fileText ?? (start === -1 ? "" : stdout.slice(start));
  if (!source.trim()) throw new Error(`${label} produced no JSON report`);
  try {
    return JSON.parse(source) as JsonReport;
  } catch (error) {
    throw new Error(`${label} produced an unparseable JSON report: ${error}`);
  }
}

function readIfPresent(target: string): string | null {
  try {
    return fs.readFileSync(target, "utf8");
  } catch {
    return null;
  }
}

export async function main(args: string[]): Promise<void> {
  const input = parseSelectionArgs(args);
  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });
  const discoveryPath = path.join(ARTIFACT_DIR, "discovery.json");
  const runPath = path.join(ARTIFACT_DIR, "run.json");
  for (const stale of [discoveryPath, runPath]) fs.rmSync(stale, { force: true });

  await assertReusableDevConsole();
  process.stdout.write(`[playwright:selection] adopted ${process.env.CT_E2E_INSTANCE ?? "playwright"}\n`);

  // --list never enters global setup, so this costs discovery and nothing else.
  const discoveryStdout = await captureStreaming(
    discoveryArgs(input),
    { ...process.env, ...REUSE_ENV, PLAYWRIGHT_JSON_OUTPUT_NAME: discoveryPath },
    DISCOVERY_TIMEOUT_MS,
  );
  const discoveryReport = parseJsonReport(readIfPresent(discoveryPath), discoveryStdout, "discovery");
  const discovered = flattenReport(discoveryReport);
  const files = repoRelativeSpecPaths(discoveryReport, discovered);
  if (files.length === 0) {
    throw new Error(
      `--project=${input.project} -g "${input.grep}" matched no tests; a zero-match selection exits 0, so it is a failure, not a pass`,
    );
  }
  process.stdout.write(
    `[playwright:selection] derived ${files.length} spec file(s), ${discovered.length} test(s):\n` +
      files.map((file) => `  ${file}\n`).join(""),
  );

  let runFailure: unknown;
  try {
    await runStreaming(
      process.execPath,
      runArgs(input, files),
      { ...process.env, ...REUSE_ENV, PLAYWRIGHT_JSON_OUTPUT_NAME: runPath },
      RUN_TIMEOUT_MS,
      "selection",
    );
  } catch (error) {
    runFailure = error;
  }

  // A missing report must never mask the run's own error, which explains it.
  const runReport = readIfPresent(runPath);
  if (runReport === null && runFailure) throw runFailure;
  const reported = flattenReport(parseJsonReport(runReport, "", "the scoped run"));
  const verdict = compareSelection(discovered, reported);
  process.stdout.write(
    `[playwright:selection] ran ${verdict.reported}/${verdict.discovered} discovered tests across ${files.length} file(s)\n`,
  );
  const shortfall = selectionFailure(verdict);
  if (shortfall) throw shortfall;
  if (runFailure) throw runFailure;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error: unknown) => {
    process.stderr.write(`[playwright:selection] ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
