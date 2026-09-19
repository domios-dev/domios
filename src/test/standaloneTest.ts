import assert from "node:assert/strict";

type TestBody = () => unknown | Promise<unknown>;
type MockFunction = ((...args: unknown[]) => unknown) & {
  mock: { calls: unknown[][] };
  mockReturnValueOnce(value: unknown): MockFunction;
  mockReturnValue(value: unknown): MockFunction;
};

const tests: Array<{ name: string; body: TestBody }> = [];
const afterEachBodies: TestBody[] = [];
const globalStubs = new Map<string, { existed: boolean; value: unknown }>();

export function describe(_name: string, body: () => void): void {
  body();
}

function register(name: string, body: TestBody): void {
  tests.push({ name, body });
}

export const it = Object.assign(register, {
  each<T>(values: readonly T[]) {
    return (name: string, body: (value: T) => unknown | Promise<unknown>) => {
      for (const value of values) register(name, () => body(value));
    };
  },
});

export function afterEach(body: TestBody): void {
  afterEachBodies.push(body);
}

function mockFunction(implementation?: (...args: unknown[]) => unknown): MockFunction {
  const once: unknown[] = [];
  let fallback = implementation;
  const fn = ((...args: unknown[]) => {
    fn.mock.calls.push(args);
    if (once.length > 0) return once.shift();
    return fallback?.(...args);
  }) as MockFunction;
  fn.mock = { calls: [] };
  fn.mockReturnValueOnce = (value) => {
    once.push(value);
    return fn;
  };
  fn.mockReturnValue = (value) => {
    fallback = () => value;
    return fn;
  };
  return fn;
}

function matchesObject(actual: unknown, expected: unknown): boolean {
  if (!expected || typeof expected !== "object") return Object.is(actual, expected);
  if (!actual || typeof actual !== "object") return false;
  return Object.entries(expected).every(([key, value]) =>
    matchesObject((actual as Record<string, unknown>)[key], value),
  );
}

function thrownMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function contains(actual: unknown, expected: unknown): boolean {
  if (typeof actual === "string") return actual.includes(String(expected));
  return Array.isArray(actual) && actual.includes(expected);
}

export function expect(actual: unknown) {
  const toThrow = (message?: string) => {
    assert.equal(typeof actual, "function");
    let thrown: unknown;
    try {
      (actual as () => unknown)();
    } catch (error) {
      thrown = error;
    }
    assert.notEqual(thrown, undefined, "expected function to throw");
    if (message) assert.match(thrownMessage(thrown), new RegExp(message));
  };
  return {
    toBe(expected: unknown) { assert.equal(actual, expected); },
    toEqual(expected: unknown) { assert.deepEqual(actual, expected); },
    toMatchObject(expected: unknown) { assert.equal(matchesObject(actual, expected), true); },
    toBeUndefined() { assert.equal(actual, undefined); },
    toBeNull() { assert.equal(actual, null); },
    toContain(expected: unknown) { assert.equal(contains(actual, expected), true); },
    toThrow,
    not: {
      toContain(expected: unknown) { assert.equal(contains(actual, expected), false); },
      toBe(expected: unknown) { assert.notEqual(actual, expected); },
      toBeNull() { assert.notEqual(actual, null); },
      toHaveBeenCalled() {
        assert.equal((actual as MockFunction).mock.calls.length, 0);
      },
    },
    rejects: {
      async toThrow(message?: string) {
        let thrown: unknown;
        try {
          await actual;
        } catch (error) {
          thrown = error;
        }
        assert.notEqual(thrown, undefined, "expected promise to reject");
        if (message) assert.match(thrownMessage(thrown), new RegExp(message));
      },
    },
  };
}

export const vi = {
  fn: mockFunction,
  stubGlobal(name: string, value: unknown) {
    if (!globalStubs.has(name)) {
      globalStubs.set(name, {
        existed: Object.prototype.hasOwnProperty.call(globalThis, name),
        value: (globalThis as Record<string, unknown>)[name],
      });
    }
    (globalThis as Record<string, unknown>)[name] = value;
  },
  unstubAllGlobals() {
    for (const [name, prior] of globalStubs) {
      if (prior.existed) (globalThis as Record<string, unknown>)[name] = prior.value;
      else delete (globalThis as Record<string, unknown>)[name];
    }
    globalStubs.clear();
  },
};

async function runTests(): Promise<void> {
  for (const test of tests) {
    try {
      await test.body();
    } catch (error) {
      const wrapped = new Error(`${test.name}: ${thrownMessage(error)}`) as Error & { cause?: unknown };
      wrapped.cause = error;
      throw wrapped;
    } finally {
      for (const body of afterEachBodies) await body();
    }
  }
}

export function finish(): void {
  void runTests().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
