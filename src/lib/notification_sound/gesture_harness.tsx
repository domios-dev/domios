/**
 * A harness for the gesture that starts the chime.
 *
 * Its audio double models the one browser rule the shipped code exists to
 * satisfy: a context constructed outside a user gesture starts SUSPENDED, a
 * suspended context makes no sound, and `resume()` only takes effect from
 * inside a gesture. Without that rule modelled, a stub would agree with a stub
 * and the silence this file is about would never be observable.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../bridge";
import { CHIME_GESTURE_EVENTS, __resetChimeContextForTest } from "../notificationSound";
import { useNotificationSound } from "../../hooks/useNotificationSound";

type ListenHandler = (event: { payload: unknown }) => void;

export function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** Depth rather than a flag: the browser's rule is "are we inside a gesture", and
 *  a handler that opens a context inside another handler is still inside one. */
let insideGesture = 0;

export interface Recorder {
  /** Every frequency that actually reached the output. Empty means silence. */
  notes: number[];
  /** How many audio contexts were constructed at all. */
  created: number;
}

function installFakeAudio(): Recorder {
  const recorder: Recorder = { notes: [], created: 0 };
  class GestureAwareContext {
    currentTime = 0;
    destination = {};
    state: string;
    constructor() {
      recorder.created += 1;
      this.state = insideGesture > 0 ? "running" : "suspended";
    }
    resume() {
      if (insideGesture > 0) this.state = "running";
    }
    createGain() {
      return {
        gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} },
        connect() {},
      };
    }
    createOscillator() {
      const context = this;
      return {
        frequency: {
          setValueAtTime: (hz: number) => {
            if (context.state === "running") recorder.notes.push(hz);
          },
          exponentialRampToValueAtTime() {},
        },
        connect() {},
        start() {},
        stop() {},
      };
    }
  }
  Object.defineProperty(globalThis, "AudioContext", {
    value: GestureAwareContext,
    configurable: true,
  });
  return recorder;
}

interface Spy {
  listening(type: string): number;
}

type AnyListener = (...args: unknown[]) => unknown;

/** Counts the app's own registrations on the window, so "it stopped listening"
 *  is an observed fact rather than an inference from a harmless second gesture. */
function installListenerSpy(target: Record<string, unknown>): Spy {
  const active = new Map<string, Set<unknown>>();
  const realAdd = target.addEventListener as AnyListener;
  const realRemove = target.removeEventListener as AnyListener;
  target.addEventListener = (type: string, handler: unknown, options?: unknown) => {
    if (!active.has(type)) active.set(type, new Set());
    active.get(type)?.add(handler);
    return realAdd.call(target, type, handler, options);
  };
  target.removeEventListener = (type: string, handler: unknown, options?: unknown) => {
    active.get(type)?.delete(handler);
    return realRemove.call(target, type, handler, options);
  };
  return { listening: (type: string) => active.get(type)?.size ?? 0 };
}

function setupDom(): Window {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  const define = (name: string, value: unknown) =>
    Object.defineProperty(globalThis, name, { value, configurable: true });
  define("window", window);
  define("document", window.document);
  define("navigator", window.navigator);
  define("HTMLElement", window.HTMLElement);
  define("SVGElement", window.SVGElement);
  define("Element", window.Element);
  define("Node", window.Node);
  define("Event", window.Event);
  return window;
}

export interface MountedGesture {
  recorder: Recorder;
  /** How many of the app's gesture listeners are still registered. */
  listening(): number;
  /** One real user gesture, dispatched on the window the app listens to. */
  gesture(type?: (typeof CHIME_GESTURE_EVENTS)[number]): Promise<void>;
  /** One daemon arrival, exactly as the daemon emits it. */
  arrive(level: string | null): Promise<void>;
  done(): Promise<void>;
}

function Harness({ enabled }: { enabled: boolean }) {
  useNotificationSound(enabled);
  return null;
}

/** Mount with the opt-in ALREADY in stored config: no settings surface is
 *  rendered and nothing is clicked, which is every restart. */
export async function mountStoredSound(enabled: boolean): Promise<MountedGesture> {
  const window = setupDom();
  insideGesture = 0;
  __resetChimeContextForTest();
  const recorder = installFakeAudio();
  const spy = installListenerSpy(window as unknown as Record<string, unknown>);
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
  await act(async () => {
    root.render(<Harness enabled={enabled} />);
  });
  await act(async () => {});

  return {
    recorder,
    listening() {
      return CHIME_GESTURE_EVENTS.reduce((total, event) => total + spy.listening(event), 0);
    },
    async gesture(type = CHIME_GESTURE_EVENTS[0]) {
      insideGesture += 1;
      try {
        await act(async () => {
          window.dispatchEvent(new window.Event(type));
        });
      } finally {
        insideGesture -= 1;
      }
    },
    async arrive(level: string | null) {
      const handler = handlers.get("notification-created");
      assert(handler, "the hook subscribes to the daemon arrival event");
      await act(async () => {
        handler({ payload: level === null ? {} : { level } });
      });
    },
    async done() {
      await act(async () => root.unmount());
      __setListenForTest(null);
    },
  };
}

export function report(name: string, assertions: number): void {
  console.log(`${name}: ${assertions} assertions passed`);
}
