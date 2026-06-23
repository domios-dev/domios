import {
  clearTerminalClientPaneUiModeOverrides,
  getClientPaneUiMode,
  normalizeTerminalPaneUiModeOverridesForChatDefault,
  resetTerminalPaneUiModeOverrideNormalization,
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

function mode(paneId: string, chatDefault: boolean) {
  return resolveInitialPaneUiMode(paneId, {
    chatCapable: true,
    chatDefault,
  });
}

test("terminal override wins when there is no chat default", () => {
  installWindow();
  setClientPaneUiMode("no-default", "terminal");
  assert(mode("no-default", false) === "terminal", "terminal override should be honored");
});

test("clearing stale terminal overrides lets chat default select chat", () => {
  installWindow();
  setClientPaneUiMode("stale-terminal", "terminal");
  setClientPaneUiMode("explicit-chat", "chat");
  window.localStorage.setItem("codeterm:pane-ui-mode:local-only", "terminal");
  window.sessionStorage.setItem("codeterm:pane-ui-mode:session-only", "terminal");
  window.localStorage.setItem("codeterm:other", "terminal");

  const cleared = clearTerminalClientPaneUiModeOverrides();

  assert(cleared.includes("stale-terminal"), "current terminal override should be cleared");
  assert(cleared.includes("local-only"), "localStorage terminal override should be cleared");
  assert(cleared.includes("session-only"), "sessionStorage terminal override should be cleared");
  assert(getClientPaneUiMode("stale-terminal") === null, "stale terminal override should be removed");
  assert(getClientPaneUiMode("explicit-chat") === "chat", "chat override should be preserved");
  assert(window.localStorage.getItem("codeterm:other") === "terminal", "unrelated keys should be preserved");
  assert(mode("stale-terminal", true) === "chat", "chat default should apply after clearing");
});

test("normalization is one-time so later terminal choices remain explicit exceptions", () => {
  installWindow();
  setClientPaneUiMode("old-terminal", "terminal");
  assert(normalizeTerminalPaneUiModeOverridesForChatDefault().includes("old-terminal"), "old terminal cleared");
  assert(mode("old-terminal", true) === "chat", "chat default applies after first normalization");

  setClientPaneUiMode("new-terminal", "terminal");
  assert(normalizeTerminalPaneUiModeOverridesForChatDefault().length === 0, "second normalization should no-op");
  assert(mode("new-terminal", true) === "terminal", "new terminal override should persist as exception");
});

test("resetting normalization makes the next chat-default enable clear terminal overrides again", () => {
  installWindow();
  normalizeTerminalPaneUiModeOverridesForChatDefault();
  setClientPaneUiMode("disabled-then-enabled", "terminal");

  resetTerminalPaneUiModeOverrideNormalization();
  assert(
    normalizeTerminalPaneUiModeOverridesForChatDefault().includes("disabled-then-enabled"),
    "terminal override created while default was off should be cleared on next enable",
  );
  assert(mode("disabled-then-enabled", true) === "chat", "chat default should apply after re-enable clear");
});

test("chat default never selects chat for a non-chat-capable pane", () => {
  installWindow();
  const resolved = resolveInitialPaneUiMode("plain-terminal", {
    chatCapable: false,
    chatDefault: true,
    chatViewFlag: true,
    mobileDefault: true,
  });
  assert(resolved === "terminal", "non-chat-capable pane should stay terminal");
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
