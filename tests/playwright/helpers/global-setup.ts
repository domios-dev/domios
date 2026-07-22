import {
  allDevInstances,
  playwrightWorkerCount,
  teardownInstances,
  type InstanceLifecycle,
} from "./dev-instance";
import { assertDevConsoleStopped, assertReusableDevConsole, ensureDevConsole, killDevConsole } from "./dev-console";

const LIFECYCLE: InstanceLifecycle = {
  kill: killDevConsole,
  assertStopped: assertDevConsoleStopped,
};

export default async function globalSetup(): Promise<void | (() => Promise<void>)> {
  const instances = allDevInstances();
  if (process.env.CT_E2E_REUSE_ONLY === "1") {
    for (const instance of instances) await assertReusableDevConsole(instance);
    return;
  }
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
    await teardownInstances(attempted, LIFECYCLE);
    throw error;
  }

  return async () => teardownInstances(instances, LIFECYCLE);
}
