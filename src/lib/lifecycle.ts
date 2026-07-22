import { IS_WEB } from "./webApi";
import { resetWebEvents } from "./webEvents";
import { fireResync } from "./resync";

const DEBOUNCE_MS = 250;
const MUX_REFRESH = "codeterm-refresh-mux-state";

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let mountedStop: (() => void) | null = null;
let sawHidden = false;

function defaultWakeResync(): void {
  resetWebEvents();
  window.dispatchEvent(new Event(MUX_REFRESH));
  fireResync();
}

let wakeResync: () => void = defaultWakeResync;

export function runWebWakeResync(): void {
  wakeResync();
}

export function setWakeResyncForTests(fn: (() => void) | null): void {
  wakeResync = fn ?? defaultWakeResync;
}

function scheduleWakeResync(): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    runWebWakeResync();
  }, DEBOUNCE_MS);
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
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = null;
    document.removeEventListener("visibilitychange", onVisibilityChange);
    window.removeEventListener("pageshow", onPageShow);
    window.removeEventListener("online", onOnline);
    mountedStop = null;
  };
  mountedStop = stop;
  return stop;
}
