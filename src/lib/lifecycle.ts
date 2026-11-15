import { IS_WEB } from "./webApi";
import { wakeWebEvents } from "./webEvents";
import { fireResync } from "./resync";
import { verifyStreamMuxes } from "./streamMux";

const MUX_REFRESH = "codeterm-refresh-mux-state";

let pendingWake: number | null = null;
let wakeGeneration = 0;
let mountedStop: (() => void) | null = null;
let sawHidden = false;

function defaultWakeResync(): void {
  window.dispatchEvent(new Event(MUX_REFRESH));
  fireResync("wake");
}

let wakeResync: () => void = defaultWakeResync;

export function runWebWakeResync(): void {
  wakeResync();
}

export function setWakeResyncForTests(fn: (() => void) | null): void {
  wakeResync = fn ?? defaultWakeResync;
}

function scheduleWakeResync(): void {
  verifyStreamMuxes();
  wakeWebEvents();
  if (pendingWake !== null) return;
  const generation = ++wakeGeneration;
  pendingWake = generation;
  queueMicrotask(() => {
    if (pendingWake !== generation) return;
    pendingWake = null;
    runWebWakeResync();
  });
}

function onVisibilityChange(): void {
  if (document.hidden) {
    sawHidden = true;
    return;
  }
  if (sawHidden) {
    sawHidden = false;
    scheduleWakeResync();
  }
}

function onPageShow(e: Event): void {
  if ((e as PageTransitionEvent).persisted) scheduleWakeResync();
}

function onOnline(): void {
  scheduleWakeResync();
}

export function mountWebLifecycle(): () => void {
  if (!IS_WEB) return () => {};
  if (mountedStop) return mountedStop;

  sawHidden = document.hidden;
  document.addEventListener("visibilitychange", onVisibilityChange);
  window.addEventListener("pageshow", onPageShow);
  window.addEventListener("online", onOnline);

  const stop = () => {
    pendingWake = null;
    document.removeEventListener("visibilitychange", onVisibilityChange);
    window.removeEventListener("pageshow", onPageShow);
    window.removeEventListener("online", onOnline);
    mountedStop = null;
  };
  mountedStop = stop;
  return stop;
}
