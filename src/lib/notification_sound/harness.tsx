/**
 * One harness for the completion sound.
 *
 * It drives the real hook through the real bridge seam and the real
 * `AudioContext` lookup, so "it sounded" is a recorded fact about the shipped
 * path rather than a stub agreeing with a stub.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../bridge";
import { useNotificationSound } from "../../hooks/useNotificationSound";

type ListenHandler = (event: { payload: unknown }) => void;

export function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom(): void {
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
}

/** Every frequency the player asked for, in call order. Empty means silence. */
export interface Recorder {
  notes: number[];
}

function installFakeAudio(): Recorder {
  const recorder: Recorder = { notes: [] };
  class RecordingContext {
    currentTime = 0;
    destination = {};
    resume() {}
    createGain() {
      return {
        gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} },
        connect() {},
      };
    }
    createOscillator() {
      return {
        frequency: {
          setValueAtTime: (hz: number) => recorder.notes.push(hz),
          exponentialRampToValueAtTime() {},
        },
        connect() {},
        start() {},
        stop() {},
      };
    }
  }
  Object.defineProperty(globalThis, "AudioContext", {
    value: RecordingContext,
    configurable: true,
  });
  return recorder;
}

export interface Mounted {
  recorder: Recorder;
  /** Deliver one daemon arrival, exactly as the bell gate emits it. */
  arrive(level: string | null): Promise<void>;
  done(): Promise<void>;
}

function Harness({ enabled }: { enabled: boolean }) {
  useNotificationSound(enabled);
  return null;
}

export async function mountSound(enabled: boolean): Promise<Mounted> {
  setupDom();
  const recorder = installFakeAudio();
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
