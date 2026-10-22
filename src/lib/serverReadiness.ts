import type { TransportClient } from "./transportClient";

export async function waitForServerReady(client: Pick<TransportClient, "subscribe">, signal: AbortSignal): Promise<void> {
  let terminal = false;
  while (!terminal) {
    await new Promise<void>((resolve, reject) => {
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
          if (settled || name !== "startup-state" || !data || typeof data !== "object" || !("state" in data)) return;
          if (data.state === "ready") { terminal = true; finish(); }
          else if (data.state === "failed") finish(new Error("Server initialization failed"));
        }, { signal, requireEventStream: true, onEnd: end => {
          if (end.reason === "eof") finish();
          else if (end.reason === "http" && end.status === 404) finish(new Error("This daemon does not support startup readiness events"));
          else finish(end.error ?? new Error(`Server startup event stream failed: ${end.reason}`));
        } });
        if (settled) dispose();
      } catch (error) { finish(error); }
    });
  }
}
