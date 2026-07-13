import { allDevInstances, playwrightWorkerCount } from "./dev-instance";
import { assertDevConsoleStopped, ensureDevConsole, killDevConsole } from "./dev-console";

export default async function globalSetup(): Promise<void | (() => Promise<void>)> {
  const instances = allDevInstances();
  if (playwrightWorkerCount() === 1 && !instances[0].name) {
    await ensureDevConsole(instances[0]);
    return;
  }

  const attempted = [];
  try {
    for (const instance of instances) {
      attempted.push(instance);
      killDevConsole(instance);
      await assertDevConsoleStopped(instance);
      await ensureDevConsole(instance);
    }
  } catch (error) {
    await stopInstances(attempted);
    throw error;
  }

  return async () => stopInstances(instances);
}

async function stopInstances(instances: ReturnType<typeof allDevInstances>): Promise<void> {
  const failures: unknown[] = [];
  for (const instance of [...instances].reverse()) {
    killDevConsole(instance);
    try {
      await assertDevConsoleStopped(instance);
    } catch (error) {
      failures.push(error);
    }
  }
  if (failures.length > 0) {
    throw new AggregateError(failures, "Playwright instance teardown leaked processes");
  }
}
