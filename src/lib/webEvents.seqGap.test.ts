// Run: npx tsx src/lib/webEvents.seqGap.test.ts
// A silent seq discontinuity is a contract violation. The client must
// self-heal AND stay loud about it, so a daemon regression cannot hide behind
// the recovery. Real frames are fed through the real module.

export {};

let failures = 0;
async function test(name: string, fn: () => Promise<void>) {
  try {
    await fn();
    console.log(`  [ok] ${name}`);
  } catch (e) {
    failures++;
    console.error(`  [x] ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

class MemoryStorage implements Storage {
  private v = new Map<string, string>();
  get length() { return this.v.size; }
  clear() { this.v.clear(); }
  getItem(k: string) { return this.v.get(k) ?? null; }
  key(i: number) { return [...this.v.keys()][i] ?? null; }
  removeItem(k: string) { this.v.delete(k); }
  setItem(k: string, val: string) { this.v.set(k, val); }
}

/** One SSE connection whose frames this test writes by hand. */
function installEnv() {
  const listeners = new Map<string, Set<EventListener>>();
  const win = {
    localStorage: new MemoryStorage(),
    sessionStorage: new MemoryStorage(),
    location: { protocol: "http:", host: "127.0.0.1:7686", origin: "http://127.0.0.1:7686" },
    addEventListener(t: string, l: EventListener) {
      const s = listeners.get(t) ?? new Set<EventListener>();
      s.add(l); listeners.set(t, s);
    },
    removeEventListener(t: string, l: EventListener) { listeners.get(t)?.delete(l); },
    dispatchEvent(e: Event) {
      for (const l of listeners.get(e.type) ?? []) l(e);
      return true;
    },
    CustomEvent,
  };
  globalThis.window = win as unknown as Window & typeof globalThis;
  globalThis.localStorage = win.localStorage;
  globalThis.sessionStorage = win.sessionStorage;

  let push!: (chunk: string) => void;
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      const enc = new TextEncoder();
      push = (chunk: string) => controller.enqueue(enc.encode(chunk));
    },
  });
  globalThis.fetch = (async () =>
    ({ ok: true, status: 200, body }) as unknown as Response) as typeof fetch;
  return { push: (frame: string) => push(frame) };
}

const frame = (seq: number, boot = "bootA"): string =>
  `data: ${JSON.stringify({ seq, boot, event_type: "tab-focused", data: { tab_id: "t" } })}\n\n`;

const settle = (): Promise<void> =>
  new Promise((r) => setTimeout(r, 0));

void (async () => {
  console.log("webEvents seq-gap detection");
  const env = installEnv();
  const webEvents = await import("./webEvents");

  let resyncRounds = 0;
  // runFullResync re-dispatches every RESYNC_EVENT with a null payload; one
  // such null on a single event is one resync.
  webEvents.webListen("plan-changed", (e) => {
    if (e.payload === null) resyncRounds++;
  });
  webEvents.webListen("tab-focused", () => {});

  await test("a contiguous stream fires no resync and counts no gap", async () => {
    const before = webEvents.webEventSeqGaps();
    for (const s of [10, 11, 12, 13, 14]) env.push(frame(s));
    await settle();
    assert(resyncRounds === 0, `contiguous stream fired ${resyncRounds} resync(s)`);
    assert(
      webEvents.webEventSeqGaps() === before,
      `contiguous stream counted ${webEvents.webEventSeqGaps() - before} gap(s)`,
    );
  });

  await test("one gap fires exactly one resync and one counter increment", async () => {
    const before = webEvents.webEventSeqGaps();
    env.push(frame(20)); // 15..19 never arrived
    await settle();
    assert(resyncRounds === 1, `expected exactly 1 resync, got ${resyncRounds}`);
    assert(
      webEvents.webEventSeqGaps() === before + 1,
      `expected exactly 1 gap counted, got ${webEvents.webEventSeqGaps() - before}`,
    );
  });

  await test("the stream stays quiet again after recovering", async () => {
    const before = webEvents.webEventSeqGaps();
    for (const s of [21, 22, 23]) env.push(frame(s));
    await settle();
    assert(resyncRounds === 1, `recovery fired an extra resync (${resyncRounds} total)`);
    assert(
      webEvents.webEventSeqGaps() === before,
      "a rebaselined stream must not keep counting gaps",
    );
  });

  await test("a duplicate frame is not a gap", async () => {
    const before = webEvents.webEventSeqGaps();
    env.push(frame(23));
    env.push(frame(22));
    await settle();
    assert(resyncRounds === 1, "duplicates must not resync");
    assert(webEvents.webEventSeqGaps() === before, "duplicates must not count as gaps");
  });

  await test("a new boot is an origin change, not a gap", async () => {
    const before = webEvents.webEventSeqGaps();
    env.push(frame(5000, "bootB"));
    await settle();
    assert(
      webEvents.webEventSeqGaps() === before,
      "a boot change must not be counted as a silent gap",
    );
  });

  console.log(failures === 0 ? "\nall passed" : `\n${failures} failed`);
  webEvents.__resetWebEventsForTest();
  process.exit(failures > 0 ? 1 : 0);
})();
