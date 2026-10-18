
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
  registeredUrls: string[] = [];
  private listeners = new Map<string, Set<() => void | Promise<void>>>();

  addEventListener(type: string, handler: () => void | Promise<void>): void {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type)!.add(handler);
  }

  async fire(type: string): Promise<void> {
    await Promise.all([...this.listeners.get(type) ?? []].map(handler => handler()));
  }

  register(scriptURL: string): Promise<{ active: null; addEventListener: () => void }> {
    this.registeredUrls.push(scriptURL);
    return Promise.resolve({ active: null, addEventListener: () => {} });
  }

  getRegistrations(): Promise<never[]> {
    return Promise.resolve([]);
  }

  getRegistration(): Promise<undefined> {
    return Promise.resolve(undefined);
  }
}

async function withFreshModule(controllerAtRegisterTime: unknown) {
  // globalThis flags it inspects, then re-import for a clean module instance.
  const win: Record<string, unknown> = globalThis as unknown as Record<string, unknown>;
  const container = new FakeServiceWorkerContainer();
  container.controller = controllerAtRegisterTime;
  const reloadCalls: number[] = [];
  const replacements: string[] = [];
  // defineProperty rather than plain assignment.
  Object.defineProperty(win, "navigator", { value: { serviceWorker: container }, configurable: true });
  Object.defineProperty(win, "location", { value: {
    href: "https://fixture.test/?tab=owned#token=opaque",
    reload: () => reloadCalls.push(1), replace: (url: string) => replacements.push(url),
  }, configurable: true });
  win.window = win;
  delete win.__TAURI_INTERNALS__;

  const mod = await import(`./serviceWorkerRegistration.ts?t=${Date.now()}-${Math.random()}`);
  mod.__resetServiceWorkerForTest();
  return { mod, container, reloadCalls, replacements };
}

function controller(build: string) {
  const requests: unknown[] = [];
  return {
    requests,
    postMessage: (message: unknown, transfer: Transferable[]) => {
      requests.push(message);
      const port = transfer[0] as MessagePort | undefined;
      queueMicrotask(() => port?.postMessage({ type: "BUILD_IDENTITY", build }));
    },
  };
}

test("first-ever controllerchange (no prior controller) does not reload", async () => {
  const { mod, container, reloadCalls } = await withFreshModule(null);
  mod.registerServiceWorker();
  container.controller = controller("dev");
  await container.fire("controllerchange");
  assert(reloadCalls.length === 0, "a fresh SW claim must not force a reload");
});

test("same-build controllerchange does not reload an already usable page", async () => {
  const oldController = controller("previous-build");
  const newController = controller("dev");
  const { mod, container, reloadCalls } = await withFreshModule(oldController);
  mod.registerServiceWorker();
  container.controller = newController;
  await container.fire("controllerchange");
  assert(reloadCalls.length === 0, "a controller matching the loaded build must not reload the page");
  assert(container.registeredUrls[0] === "./sw.js", "the worker registration keeps its stable script URL");
  assert(JSON.stringify(newController.requests) === JSON.stringify([{ type: "GET_BUILD_IDENTITY" }]), "the new controller must provide its own compiled build identity");
  assert(!mod.shouldReloadForControllerChange(true, "dev", "dev"), "the identity model must retain a same-build page");
  assert(mod.shouldReloadForControllerChange(true, "dev", null), "an unidentified controller must retain the reload fallback");
});

test("a genuine update (page already had a controller) still reloads", async () => {
  const oldController = controller("old-build");
  const newController = controller("next-build");
  const { mod, container, reloadCalls } = await withFreshModule(oldController);
  mod.registerServiceWorker();
  container.controller = newController;
  await container.fire("controllerchange");
  assert(reloadCalls.length === 1, "a real SW update must still reload once");
  assert(mod.shouldReloadForControllerChange(true, "dev", "next-build"), "the identity model must refresh for a changed build");
});

test("manual reload and a controller change share one cache-bypassing navigation", async () => {
  const oldController = controller("old-build");
  const newController = controller("next-build");
  const { mod, container, reloadCalls, replacements } = await withFreshModule(oldController);
  mod.registerServiceWorker();
  const pending = mod.reloadInterface("next-build");
  container.controller = newController;
  await container.fire("controllerchange");
  await pending;
  assert(reloadCalls.length === 0, "manual refresh must preserve its cache-bypassing navigation");
  assert(replacements.length === 1, "activation and the button must navigate only once");
  const url = new URL(replacements[0]);
  assert(url.searchParams.get("__ct_ui") === "next-build", "navigation must name the requested build");
  assert(url.searchParams.get("tab") === "owned" && url.hash === "#token=opaque", "tab and authentication must survive");
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
