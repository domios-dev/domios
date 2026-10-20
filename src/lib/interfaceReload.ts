export function interfaceReloadUrl(href: string, build: string): string {
  const url = new URL(href);
  url.searchParams.set("__ct_ui", build);
  return url.href;
}

function beforeDeadline<T>(operation: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => { cleanup(); reject(signal.reason); };
    const cleanup = () => signal.removeEventListener("abort", abort);
    signal.addEventListener("abort", abort, { once: true });
    operation.then(value => { cleanup(); resolve(value); }, error => { cleanup(); reject(error); });
    if (signal.aborted) abort();
  });
}

export async function prepareInterfaceReload(
  workers: Pick<ServiceWorkerContainer, "getRegistration"> | undefined,
  signal: AbortSignal,
): Promise<void> {
  const registration = await beforeDeadline(workers?.getRegistration() ?? Promise.resolve(undefined), signal);
  if (!registration) return;
  await beforeDeadline(registration.update(), signal);
  const worker = registration.waiting ?? registration.installing
    ?? (registration.active?.state === "activating" ? registration.active : null);
  if (!worker) return;
  await new Promise<void>((resolve, reject) => {
    let requested = false;
    const cleanup = () => {
      worker.removeEventListener("statechange", changed);
      signal.removeEventListener("abort", abort);
    };
    const abort = () => { cleanup(); reject(signal.reason); };
    const changed = () => {
      if (worker.state === "activated") { cleanup(); resolve(); }
      else if (worker.state === "redundant") { cleanup(); reject(new Error("Interface worker activation failed")); }
      else if (worker.state === "installed" && !requested) {
        requested = true;
        worker.postMessage({ type: "SKIP_WAITING" });
      }
    };
    worker.addEventListener("statechange", changed);
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort(); else changed();
  });
}
