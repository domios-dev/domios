import {
  resolveInitialPaneUiMode,
  setClientPaneUiMode,
} from "./clientPaneUiMode";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn): void {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

class MemoryStorage implements Storage {
  private values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

function installWindow(): void {
  const listeners = new Map<string, Set<EventListener>>();
  const win = {
    localStorage: new MemoryStorage(),
    sessionStorage: new MemoryStorage(),
    addEventListener(type: string, listener: EventListener) {
      const set = listeners.get(type) ?? new Set<EventListener>();
      set.add(listener);
      listeners.set(type, set);
    },
    removeEventListener(type: string, listener: EventListener) {
      listeners.get(type)?.delete(listener);
    },
    dispatchEvent(event: Event) {
      for (const listener of listeners.get(event.type) ?? []) listener(event);
      return true;
    },
  };
  globalThis.window = win as unknown as Window & typeof globalThis;
}

function mode(paneId: string) {
  return resolveInitialPaneUiMode(paneId, {
    chatCapable: true,
  });
}

test("new chat-capable Tabs open directly in chat", () => {
  installWindow();
  assert(mode("new-agent") === "chat", "chat capability should select chat");
});

test("an explicit terminal choice remains scoped to that Tab", () => {
  installWindow();
  setClientPaneUiMode("manual-terminal", "terminal");
  assert(mode("manual-terminal") === "terminal", "manual terminal choice should be honored");
  assert(mode("another-agent") === "chat", "terminal choice must not change the product default");
});

test("non-chat-capable Tabs stay in terminal", () => {
  installWindow();
  const resolved = resolveInitialPaneUiMode("plain-terminal", {
    chatCapable: false,
  });
  assert(resolved === "terminal", "non-chat-capable Tab should stay terminal");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(err);
  }
}

console.log(`clientPaneUiMode: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
