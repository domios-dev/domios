import type { TransportClient } from "./transportClient";

export function startingSubsystem(response: Response, value: unknown, requireUnexecuted = false): string | null {
  if (response.status !== 503 || !value || typeof value !== "object" || !("error" in value)) return null;
  const error = value.error;
  if (!error || typeof error !== "object" || !("code" in error) || error.code !== "starting"
    || !("subsystem" in error) || typeof error.subsystem !== "string") return null;
  if (requireUnexecuted && (!("request_executed" in error) || error.request_executed !== false)) return null;
  return error.subsystem;
}

export function createStartupLoading(subscribe: TransportClient["subscribe"]) {
  const waiters = new Set<{ area: string; finish(error?: unknown): void }>();
  let close: (() => void) | undefined;
  let opening = false;
  const stop = () => { close?.(); close = undefined; };
  return {
    wait(area: string, signal?: AbortSignal): Promise<void> {
      return new Promise((resolve, reject) => {
        let finished = false;
        const waiter = { area, finish(error?: unknown) {
          if (finished) return;
          finished = true;
          signal?.removeEventListener("abort", abort);
          waiters.delete(waiter);
          if (!waiters.size) stop();
          if (error) reject(error); else resolve();
        } };
        const abort = () => waiter.finish(signal?.reason ?? new Error("Startup request cancelled"));
        if (signal?.aborted) { abort(); return; }
        signal?.addEventListener("abort", abort, { once: true });
        waiters.add(waiter);
        if (close || opening) return;
        opening = true;
        try {
          const dispose = subscribe("/api/startup/events", (name, value) => {
            if (name !== "startup-state" || !value || typeof value !== "object" || !("state" in value)) return;
            const areas = "subsystems" in value && value.subsystems && typeof value.subsystems === "object"
              ? value.subsystems as Record<string, unknown> : {};
            for (const pending of [...waiters]) {
              if (value.state === "failed" || areas[pending.area] === "failed") pending.finish(new Error(`${pending.area} initialization failed`));
              else if (areas[pending.area] !== "starting"
                && (value.state === "ready" || ("routes_ready" in value && value.routes_ready === true))) pending.finish();
            }
          }, { requireEventStream: true, onEnd: end => {
            for (const pending of [...waiters]) pending.finish(end.error ?? new Error(`Startup stream ended: ${end.reason}`));
          } });
          if (waiters.size) close = dispose;
          else dispose();
        } catch (error) {
          for (const pending of [...waiters]) pending.finish(error);
        } finally { opening = false; }
      });
    },
  };
}
