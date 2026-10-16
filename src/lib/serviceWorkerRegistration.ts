import { IS_WEB } from "./webApi";
import { interfaceReloadUrl, prepareInterfaceReload } from "./interfaceReload";

const REG_FAIL_KEY = "codeterm-sw-reg-failures";
const MAX_REG_FAILS = 3;
const SW_PATH = "./sw.js";

let refreshing = false;
let reloadTarget: string | null = null;
let reloadPending: Promise<void> | null = null;

export function __resetServiceWorkerForTest(): void {
  refreshing = false;
  reloadTarget = null;
  reloadPending = null;
}

function reloadPageOnce(): void {
  if (refreshing) return;
  refreshing = true;
  if (reloadTarget) window.location.replace(reloadTarget);
  else window.location.reload();
}

export function reloadInterface(build: string): Promise<void> {
  if (reloadPending) return reloadPending;
  reloadTarget = interfaceReloadUrl(window.location.href, build);
  reloadPending = prepareInterfaceReload(navigator.serviceWorker, AbortSignal.timeout(15000))
    .then(reloadPageOnce).finally(() => { reloadPending = null; reloadTarget = null; });
  return reloadPending;
}

function recordRegistrationFailure(): void {
  try {
    const fails = Number.parseInt(localStorage.getItem(REG_FAIL_KEY) ?? "0", 10) + 1;
    localStorage.setItem(REG_FAIL_KEY, String(fails));
    if (fails >= MAX_REG_FAILS) {
      void navigator.serviceWorker.getRegistrations().then((regs) => {
        void Promise.all(regs.map((r) => r.unregister()));
        localStorage.removeItem(REG_FAIL_KEY);
      });
    }
  } catch {
  }
}

function clearRegistrationFailures(): void {
  try {
    localStorage.removeItem(REG_FAIL_KEY);
  } catch {
  }
}

function activateWaitingWorker(registration: ServiceWorkerRegistration): void {
  if (!registration.waiting) return;
  registration.waiting.postMessage({ type: "SKIP_WAITING" });
}

function watchForUpdates(registration: ServiceWorkerRegistration): void {
  registration.addEventListener("updatefound", () => {
    const worker = registration.installing;
    if (!worker) return;
    worker.addEventListener("statechange", () => {
      if (worker.state === "installed" && navigator.serviceWorker.controller) {
        activateWaitingWorker(registration);
      }
    });
  });

  if (registration.waiting && navigator.serviceWorker.controller) {
    activateWaitingWorker(registration);
  }
}

export function registerServiceWorker(): void {
  if (!IS_WEB || !("serviceWorker" in navigator)) return;

  const hadController = Boolean(navigator.serviceWorker.controller);
  navigator.serviceWorker.addEventListener("controllerchange", () => {
    if (!hadController) return;
    reloadPageOnce();
  });

  void navigator.serviceWorker
    .register(SW_PATH, { scope: "./" })
    .then((registration) => {
      clearRegistrationFailures();
      watchForUpdates(registration);
      if (registration.active) watchForUpdates(registration);
    })
    .catch(() => {
      recordRegistrationFailure();
    });
}
