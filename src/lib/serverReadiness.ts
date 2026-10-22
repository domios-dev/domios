import type { TransportClient } from "./transportClient";

export function waitForServerReady(client: Pick<TransportClient, "subscribe">, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false;
    let dispose: (() => void) | undefined;
    const finish = (error?: unknown) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener("abort", abort);
      dispose?.();
      if (error) reject(error); else resolve();
    };
    const abort = () => finish(signal.reason ?? new Error("Startup wait cancelled"));
    if (signal.aborted) { abort(); return; }
    signal.addEventListener("abort", abort, { once: true });
    try {
      dispose = client.subscribe("/api/startup/events", (name, data) => {
        if (name !== "startup-state" || !data || typeof data !== "object" || !("state" in data)) return;
        if (data.state === "ready") finish();
        else if (data.state === "failed") finish(new Error("Server initialization failed"));
      }, { signal, onEnd: () => finish(new Error("Server startup event stream ended before readiness")) });
      if (settled) dispose();
    } catch (error) { finish(error); }
  });
}
