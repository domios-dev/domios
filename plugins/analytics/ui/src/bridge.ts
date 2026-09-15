import type { AnalyticsQuery } from "@codeterm/plugin-sdk";
import { KPI_CHANGE_NOTICE } from "../../../../src/lib/kpiChanges";
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

export type HostNotice = "shown" | "hidden" | "kpi_changed";

/** Visibility and KPI change notices the host posts to this kept-mounted view. */
export function onHostNotice(callback: (notice: HostNotice) => void): () => void {
  const onMessage = (event: MessageEvent) => {
    if (event.source !== window.parent) return;
    const data = event.data as Record<string, unknown> | null;
    if (data?.__ctActive === true) callback("shown");
    else if (data?.__ctActive === false) callback("hidden");
    else if (data?.[KPI_CHANGE_NOTICE] === true) callback("kpi_changed");
  };
  window.addEventListener("message", onMessage);
  return () => window.removeEventListener("message", onMessage);
}
