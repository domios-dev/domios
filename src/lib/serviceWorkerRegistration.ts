import { IS_WEB } from "./webApi";
import { buildHash } from "./buildStamp";
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

export function shouldReloadForControllerChange(
  hadController: boolean,
  loadedBuild: string,
  nextControllerBuild: string | null,
): boolean {
  return hadController && nextControllerBuild !== loadedBuild;
}

function controllerBuild(controller: ServiceWorker | null): Promise<string | null> {
  if (!controller) return Promise.resolve(null);
  let channel: MessageChannel;
  try {
    channel = new MessageChannel();
  } catch {
    return Promise.resolve(null);
  }
  const signal = AbortSignal.timeout(5000);
  return new Promise((resolve) => {
    let settled = false;
    const cleanup = () => {
      signal.removeEventListener("abort", onAbort);
      channel.port1.removeEventListener("message", onMessage);
      channel.port1.removeEventListener("messageerror", onAbort);
      channel.port1.close();
      channel.port2.close();
    };
    const finish = (build: string | null) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(build);
    };
    const onAbort = () => finish(null);
    const onMessage = (event: MessageEvent<unknown>) => {
      if (!event.data || typeof event.data !== "object") return finish(null);
      const message = event.data as { type?: unknown; build?: unknown };
      finish(message.type === "BUILD_IDENTITY" && typeof message.build === "string" ? message.build : null);
    };
    channel.port1.addEventListener("message", onMessage, { once: true });
    channel.port1.addEventListener("messageerror", onAbort, { once: true });
    signal.addEventListener("abort", onAbort, { once: true });
    channel.port1.start();
    if (signal.aborted) return onAbort();
    try {
      controller.postMessage({ type: "GET_BUILD_IDENTITY" }, [channel.port2]);
    } catch {
      finish(null);
    }
  });
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
  const loadedBuild = buildHash();
  navigator.serviceWorker.addEventListener("controllerchange", async () => {
    if (!hadController) return;
    const nextController = navigator.serviceWorker.controller;
    const nextBuild = await controllerBuild(nextController);
    if (navigator.serviceWorker.controller !== nextController) return;
    if (!shouldReloadForControllerChange(hadController, loadedBuild, nextBuild)) return;
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
