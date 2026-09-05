// A test file with no top-level `import`/`export` is a global SCRIPT to TS,
// so its helpers collide with every sibling script's helpers. Keep this.
export {};

function check(condition: unknown, message: string): asserts condition {
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

function installBrowser(hidden = false): {
  setHidden: (next: boolean) => void;
  muxRefreshCount: () => number;
} {
  const docListeners = new Map<string, Set<EventListener>>();
  const winListeners = new Map<string, Set<EventListener>>();
  let hiddenState = hidden;
  let muxRefreshes = 0;

  const document = {
    get hidden() {
      return hiddenState;
    },
    addEventListener(type: string, listener: EventListener) {
      const set = docListeners.get(type) ?? new Set<EventListener>();
      set.add(listener);
      docListeners.set(type, set);
    },
    removeEventListener(type: string, listener: EventListener) {
      docListeners.get(type)?.delete(listener);
    },
    dispatchEvent(event: Event) {
      for (const listener of docListeners.get(event.type) ?? []) listener(event);
      return true;
    },
  };

  const window = {
    localStorage: new MemoryStorage(),
    sessionStorage: new MemoryStorage(),
    location: { protocol: "http:", host: "127.0.0.1:7686", origin: "http://127.0.0.1:7686" },
    addEventListener(type: string, listener: EventListener) {
      const set = winListeners.get(type) ?? new Set<EventListener>();
      set.add(listener);
      winListeners.set(type, set);
    },
    removeEventListener(type: string, listener: EventListener) {
      winListeners.get(type)?.delete(listener);
    },
    dispatchEvent(event: Event) {
      if (event.type === "codeterm-refresh-mux-state") muxRefreshes++;
      for (const listener of winListeners.get(event.type) ?? []) listener(event);
      return true;
    },
  };

  globalThis.window = window as unknown as Window & typeof globalThis;
  globalThis.document = document as unknown as Document;
  globalThis.localStorage = window.localStorage;
  globalThis.sessionStorage = window.sessionStorage;
  globalThis.fetch = async () =>
    ({ ok: false, status: 503, body: null }) as unknown as Response;

  if (typeof PageTransitionEvent === "undefined") {
    class FakePageTransitionEvent extends Event {
      persisted: boolean;
      constructor(type: string, init?: { persisted?: boolean }) {
        super(type);
        this.persisted = init?.persisted ?? false;
      }
    }
    (globalThis as unknown as { PageTransitionEvent: typeof PageTransitionEvent }).PageTransitionEvent =
      FakePageTransitionEvent as unknown as typeof PageTransitionEvent;
  }

  return {
    setHidden(next: boolean) {
      hiddenState = next;
      document.dispatchEvent(new Event("visibilitychange"));
    },
    muxRefreshCount: () => muxRefreshes,
  };
}

void (async () => {
  installBrowser(false);
  const { mountWebLifecycle, setWakeResyncForTests } = await import("./lifecycle");
  setWakeResyncForTests(() => {
    window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
  });
  let passed = 0;
  let failed = 0;

  async function runCase(name: string, fn: () => Promise<void>) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
      passed++;
    } catch (e) {
      console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : e}`);
      failed++;
    }
  }

  await runCase("hidden→visible schedules debounced mux refresh", async () => {
    const env = installBrowser(false);
    const stop = mountWebLifecycle();
    try {
      env.setHidden(true);
      env.setHidden(false);
      check(env.muxRefreshCount() === 0, "resync should be debounced immediately");
      await new Promise((r) => setTimeout(r, 300));
      check(env.muxRefreshCount() === 1, "wake resync dispatches mux refresh once");
    } finally {
      stop();
    }
  });

  await runCase("visible without prior hidden does not resync", async () => {
    const env = installBrowser(false);
    const stop = mountWebLifecycle();
    try {
      env.setHidden(false);
      await new Promise((r) => setTimeout(r, 300));
      check(env.muxRefreshCount() === 0, "no resync without a hidden period");
    } finally {
      stop();
    }
  });

  await runCase("pageshow persisted schedules debounced resync", async () => {
    const env = installBrowser(false);
    const stop = mountWebLifecycle();
    try {
      window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
      await new Promise((r) => setTimeout(r, 300));
      check(env.muxRefreshCount() === 1, "bfcache pageshow triggers resync");
    } finally {
      stop();
    }
  });

  await runCase("online schedules debounced resync", async () => {
    const env = installBrowser(false);
    const stop = mountWebLifecycle();
    try {
      window.dispatchEvent(new Event("online"));
      await new Promise((r) => setTimeout(r, 300));
      check(env.muxRefreshCount() === 1, "online triggers resync");
    } finally {
      stop();
    }
  });

  await runCase("rapid wake signals coalesce to one resync", async () => {
    const env = installBrowser(false);
    const stop = mountWebLifecycle();
    try {
      window.dispatchEvent(new Event("online"));
      window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
      await new Promise((r) => setTimeout(r, 300));
      check(env.muxRefreshCount() === 1, "debounce coalesces multiple wake signals");
    } finally {
      stop();
    }
  });

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
})();
