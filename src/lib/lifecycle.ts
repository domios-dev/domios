import { IS_WEB } from "./webApi";
import { wakeWebEvents } from "./webEvents";
import { verifyStreamMuxes } from "./streamMux";

let mountedStop: (() => void) | null = null;
let sawHidden = false;

function wakeConnection(): void {
  void verifyStreamMuxes();
  wakeWebEvents();
}

function onVisibilityChange(): void {
  if (document.hidden) {
    sawHidden = true;
    return;
  }
  if (sawHidden) {
    sawHidden = false;
    wakeConnection();
  }
}

function onPageShow(e: Event): void {
  if ((e as PageTransitionEvent).persisted) wakeConnection();
}

function onOnline(): void {
  wakeConnection();
}

export function mountWebLifecycle(): () => void {
  if (!IS_WEB) return () => {};
  if (mountedStop) return mountedStop;

  sawHidden = document.hidden;
  document.addEventListener("visibilitychange", onVisibilityChange);
  window.addEventListener("pageshow", onPageShow);
  window.addEventListener("online", onOnline);

  const stop = () => {
    document.removeEventListener("visibilitychange", onVisibilityChange);
    window.removeEventListener("pageshow", onPageShow);
    window.removeEventListener("online", onOnline);
    mountedStop = null;
  };
  mountedStop = stop;
  return stop;
}
