
export {}; // force module scope — this file's `test`/`main` are otherwise
// global-script declarations that collide with every other homegrown
// `.test.ts` harness file under `tsc --noEmit`'s whole-project check.

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

class FakeServiceWorkerContainer {
  controller: unknown;
  private listeners = new Map<string, Set<() => void>>();

  addEventListener(type: string, handler: () => void): void {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type)!.add(handler);
  }

  fire(type: string): void {
    for (const handler of this.listeners.get(type) ?? []) handler();
  }

  register(): Promise<{ active: null; addEventListener: () => void }> {
    return Promise.resolve({ active: null, addEventListener: () => {} });
  }

  getRegistrations(): Promise<never[]> {
    return Promise.resolve([]);
  }
}

async function withFreshModule(controllerAtRegisterTime: unknown) {
  // globalThis flags it inspects, then re-import for a clean module instance.
  const win: Record<string, unknown> = globalThis as unknown as Record<string, unknown>;
  const container = new FakeServiceWorkerContainer();
  container.controller = controllerAtRegisterTime;
  const reloadCalls: number[] = [];
  // defineProperty rather than plain assignment.
  Object.defineProperty(win, "navigator", { value: { serviceWorker: container }, configurable: true });
  Object.defineProperty(win, "location", { value: { reload: () => reloadCalls.push(1) }, configurable: true });
  win.window = win;
  delete win.__TAURI_INTERNALS__;

  const mod = await import(`./serviceWorkerRegistration.ts?t=${Date.now()}-${Math.random()}`);
  return { mod, container, reloadCalls };
}

test("first-ever controllerchange (no prior controller) does not reload", async () => {
  const { mod, container, reloadCalls } = await withFreshModule(null);
  mod.registerServiceWorker();
  container.fire("controllerchange");
  assert(reloadCalls.length === 0, "a fresh SW claim must not force a reload");
});

test("a genuine update (page already had a controller) still reloads", async () => {
  const { mod, container, reloadCalls } = await withFreshModule({ id: "old-worker" });
  mod.registerServiceWorker();
  container.fire("controllerchange");
  assert(reloadCalls.length === 1, "a real SW update must still reload once");
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`serviceWorkerRegistration tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
