import type { TransportClient } from "./transportClient";

export const startupPhases = ["listener", "integrity", "states", "providers", "service", "plans", "background", "platform"] as const;
export type StartupPhase = typeof startupPhases[number];

export function currentStartupPhase(snapshot: unknown): StartupPhase | null {
  if (!snapshot || typeof snapshot !== "object" || !("state" in snapshot) || snapshot.state !== "starting"
    || !("phases" in snapshot) || !snapshot.phases || typeof snapshot.phases !== "object") return null;
  const phases = snapshot.phases as Record<string, unknown>;
  return startupPhases.find(phase => phases[phase] === "starting") ?? null;
}

export async function waitForDesktopReady(client: Pick<TransportClient, "subscribe">, signal: AbortSignal, onPhase?: (phase: StartupPhase | null) => void): Promise<void> {
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
          onPhase?.(currentStartupPhase(data));
          if (data.state === "ready" || (data.state === "starting" && "desktop_ready" in data && data.desktop_ready === true)) { terminal = true; finish(); }
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
