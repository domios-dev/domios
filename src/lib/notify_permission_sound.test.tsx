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
import type { AgentLevelChoice } from "./types";

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

/** One reading of the counters, taken now. */
interface Observation {
  built: number;
  resumed: number;
  notes: number;
  state: string | null;
}

/**
 * The counters advance inside the audio callbacks, where control-flow analysis
 * cannot follow, so `probe` itself is never read twice in one flow: an assertion
 * about it would narrow the field to a literal and make the next one impossible.
 * Every assertion below is about a snapshot, which really is that literal.
 */
function observed(): Observation {
  return {
    built: probe.built,
    resumed: probe.resumed,
    notes: probe.notes.length,
    state: probe.last?.state ?? null,
  };
}

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

/** Rows as the daemon ships them: present, and answering nothing for the chime,
 *  so each level falls back to what shipped. Which levels chime is level_sound's
 *  claim; this file is about the browser's permission. */
const LEVELS: AgentLevelChoice[] = ["verbose", "action_needed", "failure"].map((level) => ({
  level,
  bell: true,
  desktop: false,
}));

function Harness({ enabled }: { enabled: boolean }) {
  useNotificationSound(enabled, LEVELS);
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
  const afterGesture = observed();
  check(afterGesture.built === 1, `and opens exactly one, got ${afterGesture.built}`);
  check(
    afterGesture.resumed === 1,
    `a context born suspended is resumed inside the gesture, got ${afterGesture.resumed} resume(s)`,
  );
  check(afterGesture.state === "running", "so the context is running when the gesture ends");

  check(
    establishChimeContext() === established,
    "a second gesture reuses the established context rather than opening another",
  );
  const afterSecondGesture = observed();
  check(
    afterSecondGesture.built === 1,
    `and opens no second one, got ${afterSecondGesture.built}`,
  );
  check(
    afterSecondGesture.resumed === 1,
    `a running context is not resumed again — the state is checked, not assumed, got ${afterSecondGesture.resumed} resume(s)`,
  );

  const listener = await mountListener();
  await listener.arrive("action_needed");
  const afterArrival = observed();
  check(
    afterArrival.notes > 0,
    "the very next action_needed sounds, with no second click to open a context",
  );
  check(
    afterArrival.built === 1,
    `and it sounds on the context the gesture established, got ${afterArrival.built} context(s)`,
  );

  const context = probe.last;
  check(context, "the probe recorded the context the gesture established");
  context.state = "suspended";
  await listener.arrive("action_needed");
  const afterResuspension = observed();
  check(
    afterResuspension.resumed === 2,
    `a context the host suspended later is resumed rather than left silent, got ${afterResuspension.resumed} resume(s)`,
  );
  check(
    afterResuspension.notes > afterArrival.notes,
    "and that arrival is audible instead of being dropped into a suspended context",
  );

  await listener.done();
  console.log(`notify_permission/sound_established_by_the_gesture: ${checked} assertions passed`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
