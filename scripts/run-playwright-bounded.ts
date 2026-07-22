import path from "node:path";
import { fileURLToPath } from "node:url";
import { killDevConsole, assertDevConsoleStopped } from "../tests/playwright/helpers/dev-console";
import {
  devInstanceForSlot,
  teardownInstances,
  type DevInstance,
  type InstanceLifecycle,
} from "../tests/playwright/helpers/dev-instance";
import {
  parsePlaywrightSpec,
  runStreaming,
} from "./run-playwright-isolated";

const TOTAL_BUDGET_MS = 600_000;
const PER_INSTANCE_CLEANUP_MS = 95_000;
const CHILD_EXIT_HEADROOM_MS = 15_000;
const TEST_TIMEOUT_MS = 30_000;
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PLAYWRIGHT_CLI = path.join(REPO_ROOT, "node_modules", "@playwright", "test", "cli.js");

export interface BoundedPlaywrightArgs {
  files: string[];
  grep?: string;
  project: string;
  workers: number;
}

export interface PlaywrightBudgets {
  childDeadlineMs: number;
  innerGlobalTimeoutMs: number;
  totalBudgetMs: number;
}

function optionValues(args: string[], name: string): string[] {
  const values: string[] = [];
  for (let index = 0; index < args.length; index += 2) {
    if (args[index] === name) values.push(args[index + 1]);
  }
  return values;
}

function singleOption(args: string[], name: string, required: boolean): string | undefined {
  const values = optionValues(args, name);
  if (values.length > 1 || (required && values.length !== 1) || values.some((value) => !value?.trim())) {
    throw new Error(`${name} must be supplied ${required ? "exactly once" : "at most once"} with a non-empty value`);
  }
  return values[0];
}

export function parseBoundedPlaywrightArgs(args: string[]): BoundedPlaywrightArgs {
  const allowed = new Set(["--file", "--grep", "--project", "--workers"]);
  if (args.length === 0 || args.length % 2 !== 0) throw new Error("options must be supplied as name/value pairs");
  for (let index = 0; index < args.length; index += 2) {
    if (!allowed.has(args[index])) throw new Error(`unsupported option ${args[index]}`);
  }
  const files = optionValues(args, "--file").map(parsePlaywrightSpec);
  if (files.length === 0) throw new Error("--file must be supplied at least once");
  const project = singleOption(args, "--project", true)!;
  const grep = singleOption(args, "--grep", false);
  const workers = Number(singleOption(args, "--workers", true));
  if (!Number.isInteger(workers) || workers < 1) throw new Error("--workers must be a positive integer");
  budgetsForWorkers(workers);
  return { files, grep, project, workers };
}

export function budgetsForWorkers(workers: number): PlaywrightBudgets {
  const childDeadlineMs = TOTAL_BUDGET_MS - workers * PER_INSTANCE_CLEANUP_MS;
  const innerGlobalTimeoutMs = childDeadlineMs - CHILD_EXIT_HEADROOM_MS;
  if (innerGlobalTimeoutMs < TEST_TIMEOUT_MS) {
    throw new Error(`${workers} workers leave no bounded Playwright execution window`);
  }
  return { childDeadlineMs, innerGlobalTimeoutMs, totalBudgetMs: TOTAL_BUDGET_MS };
}

export function boundedPlaywrightArgs(input: BoundedPlaywrightArgs): string[] {
  const budgets = budgetsForWorkers(input.workers);
  const files = input.files.map((file) => path.relative(REPO_ROOT, file).replaceAll("\\", "/"));
  return [
    PLAYWRIGHT_CLI,
    "test",
    "--config=tests/playwright/playwright.config.ts",
    `--project=${input.project}`,
    ...files,
    ...(input.grep ? ["-g", input.grep] : []),
    `--workers=${input.workers}`,
    `--timeout=${TEST_TIMEOUT_MS}`,
    `--global-timeout=${budgets.innerGlobalTimeoutMs}`,
    "--reporter=line,json",
  ];
}

function appendFailure(target: unknown[], error: unknown): void {
  if (error instanceof AggregateError) target.push(...error.errors);
  else target.push(error);
}

export async function runWithGuaranteedCleanup(
  runChild: () => Promise<void>,
  instances: DevInstance[],
  lifecycle: InstanceLifecycle,
): Promise<void> {
  const failures: unknown[] = [];
  try {
    await runChild();
  } catch (error) {
    appendFailure(failures, error);
  } finally {
    try {
      await teardownInstances(instances, lifecycle);
    } catch (error) {
      appendFailure(failures, error);
    }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "Playwright run and cleanup failed");
}

const LIFECYCLE: InstanceLifecycle = {
  kill: killDevConsole,
  assertStopped: assertDevConsoleStopped,
};

export async function main(args: string[]): Promise<void> {
  const input = parseBoundedPlaywrightArgs(args);
  const budgets = budgetsForWorkers(input.workers);
  const instances = Array.from({ length: input.workers }, (_, slot) => devInstanceForSlot(slot, input.workers));
  process.stdout.write(
    `[playwright:bounded] workers=${input.workers} inner=${budgets.innerGlobalTimeoutMs}ms cleanup=${budgets.totalBudgetMs - budgets.childDeadlineMs}ms\n`,
  );
  await runWithGuaranteedCleanup(
    () => runStreaming(
      process.execPath,
      boundedPlaywrightArgs(input),
      { ...process.env, CT_E2E_WORKERS: String(input.workers) },
      budgets.childDeadlineMs,
      "bounded",
    ),
    instances,
    LIFECYCLE,
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error: unknown) => {
    process.stderr.write(`[playwright:bounded] ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
