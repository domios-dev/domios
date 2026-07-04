import { IS_WEB } from "./webApi";

const REG_FAIL_KEY = "codeterm-sw-reg-failures";
const MAX_REG_FAILS = 3;
const SW_PATH = "./sw.js";

let refreshing = false;

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
    // storage unavailable — skip kill-switch bookkeeping
  }
}

function clearRegistrationFailures(): void {
  try {
    localStorage.removeItem(REG_FAIL_KEY);
  } catch {
    // ignore
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

  // A page with NO prior controller (first-ever visit, or a hard refresh
  // after the SW was gone) still fires exactly one "controllerchange" the
  // moment the newly-registered worker claims it. That is not an update —
  // reloading here would abort whatever the user is mid-doing (e.g.
  // confirming the new-session picker) for no reason. Only a REAL update —
  // an already-controlled page whose worker changes underneath it — needs
  // the reload, since stale cached assets could otherwise mismatch the API.
  const hadController = Boolean(navigator.serviceWorker.controller);
  navigator.serviceWorker.addEventListener("controllerchange", () => {
    if (!hadController || refreshing) return;
    refreshing = true;
    window.location.reload();
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
