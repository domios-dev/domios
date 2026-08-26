/**
 * Stand up (or repair) the named Playwright dev instance and leave it running.
 * The adopt-only runners refuse to start one; this is the command they name.
 */
import { assertReusableDevConsole, ensureDevConsole } from "../tests/playwright/helpers/dev-console";
import { currentDevInstance } from "../tests/playwright/helpers/dev-instance";

export async function main(): Promise<void> {
  const instance = currentDevInstance();
  await ensureDevConsole(instance);
  await assertReusableDevConsole(instance);
  process.stdout.write(
    `[playwright:instance] ${instance.name || "default"} is standing at ${instance.apiBase} (CDP ${instance.cdpBase})\n`,
  );
}

if (process.argv[1]?.endsWith("start-playwright-instance.ts")) {
  main().catch((error: unknown) => {
    process.stderr.write(`[playwright:instance] ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
