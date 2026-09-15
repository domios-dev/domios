import type { AnalyticsQuery } from "@codeterm/plugin-sdk";
import type { QueryOutcome } from "../../src/plugin";

interface CtBridge {
  invoke(method: string, args?: unknown): Promise<unknown>;
  close(): void;
}

function bridge(): CtBridge | undefined {
  return (window as unknown as { ct?: CtBridge }).ct;
}

function isOutcome(value: unknown): value is QueryOutcome {
  const kind = (value as { kind?: unknown } | null)?.kind;
  return kind === "snapshot" || kind === "refused" || kind === "unavailable";
}

/** A failed or malformed bridge reply reads as "no answer", never as an empty snapshot. */
export async function queryAnalytics(query: AnalyticsQuery): Promise<QueryOutcome> {
  const ct = bridge();
  if (!ct) return { kind: "unavailable" };
  try {
    const reply = await ct.invoke("query", { query });
    return isOutcome(reply) ? reply : { kind: "unavailable" };
  } catch {
    return { kind: "unavailable" };
  }
}

export function closeView(): void {
  bridge()?.close();
}

/** The host tells a kept-mounted view when it is shown again. */
export function onViewShown(callback: () => void): () => void {
  const onMessage = (event: MessageEvent) => {
    if (event.source !== window.parent) return;
    if ((event.data as { __ctActive?: unknown } | null)?.__ctActive === true) callback();
  };
  window.addEventListener("message", onMessage);
  return () => window.removeEventListener("message", onMessage);
}
