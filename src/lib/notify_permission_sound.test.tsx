/**
 * Run: npm run test:unit -- notify_permission
 *
 * Permission for the chime is the browser's, and it is established by the
 * gesture that enables the sound. A context opened outside a gesture starts
 * suspended, and a suspended context is silence — so enabling must open and
 * resume it there, and the very next arrival must sound on that same context
 * with no second click.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "./bridge";
import { __resetChimeContextForTest, establishChimeContext } from "./notificationSound";
import { useNotificationSound } from "../hooks/useNotificationSound";

let checked = 0;
function check(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ]) {
    Object.defineProperty(globalThis, key, {
      value: window[key as keyof Window],
      configurable: true,
    });
  }
}

interface Probe {
  built: number;
  resumed: number;
  notes: number[];
  last: { state: string } | null;
}

const probe: Probe = { built: 0, resumed: 0, notes: [], last: null };

/** A context that behaves like a real one born outside a gesture: suspended. */
class SuspendedAudioContext {
  currentTime = 0;
  destination = {};
  state = "suspended";

  constructor() {
    probe.built += 1;
    probe.last = this;
  }

  resume() {
    probe.resumed += 1;
    this.state = "running";
    return Promise.resolve();
  }

  createGain() {
    return {
      gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} },
      connect() {},
    };
  }

  createOscillator() {
    return {
      frequency: {
        setValueAtTime: (hz: number) => probe.notes.push(hz),
        exponentialRampToValueAtTime() {},
      },
      connect() {},
      start() {},
      stop() {},
    };
  }
}

setupDom();
Object.defineProperty(globalThis, "AudioContext", {
  value: SuspendedAudioContext,
  configurable: true,
});

type ListenHandler = (event: { payload: unknown }) => void;

/** Drain the promise chain the subscription awaits. Microtasks only — nothing timed. */
async function settle(): Promise<void> {
  for (let turn = 0; turn < 4; turn++) await act(async () => {});
}

function Harness({ enabled }: { enabled: boolean }) {
  useNotificationSound(enabled);
  return null;
}

/** The real hook over the real bridge seam, mounted after the enabling gesture. */
async function mountListener() {
  const handlers = new Map<string, ListenHandler>();
  __setListenForTest(async (event, handler) => {
    handlers.set(event, handler as ListenHandler);
    return () => {
      handlers.delete(event);
    };
  });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(<Harness enabled />));
  await settle();
  return {
    async arrive(level: string) {
      const handler = handlers.get("notification-created");
      check(handler, "the hook subscribes to the daemon arrival event");
      await act(async () => handler({ payload: { level } }));
    },
    async done() {
      await act(async () => root.unmount());
      host.remove();
      __setListenForTest(null);
    },
  };
}

async function main() {
  __resetChimeContextForTest();

  const established = establishChimeContext();
  check(established, "the enabling gesture opens an audio context");
  check(probe.built === 1, `and opens exactly one, got ${probe.built}`);
  check(
    probe.resumed === 1,
    `a context born suspended is resumed inside the gesture, got ${probe.resumed} resume(s)`,
  );
  check(probe.last?.state === "running", "so the context is running when the gesture ends");

  check(
    establishChimeContext() === established && probe.built === 1,
    "a second gesture reuses the established context rather than opening another",
  );
  check(
    probe.resumed === 1,
    "and a running context is not resumed again — the state is checked, not assumed",
  );

  const listener = await mountListener();
  await listener.arrive("action_needed");
  check(
    probe.notes.length > 0,
    "the very next action_needed sounds, with no second click to open a context",
  );
  check(
    probe.built === 1,
    `and it sounds on the context the gesture established, got ${probe.built} context(s)`,
  );

  const beforeSuspension = probe.notes.length;
  probe.last!.state = "suspended";
  await listener.arrive("action_needed");
  check(
    probe.resumed === 2,
    `a context the host suspended later is resumed rather than left silent, got ${probe.resumed} resume(s)`,
  );
  check(
    probe.notes.length > beforeSuspension,
    "and that arrival is audible instead of being dropped into a suspended context",
  );

  await listener.done();
  console.log(`notify_permission/sound_established_by_the_gesture: ${checked} assertions passed`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
