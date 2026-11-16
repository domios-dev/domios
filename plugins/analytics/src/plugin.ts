import { defineProviderIo, ioValue, type HostIoSteps, type IoExports, type AnalyticsQuery, type AnalyticsSnapshot, type PluginModule } from "@codeterm/plugin-sdk";

/** What the view receives: a snapshot, the host's refusal, or no answer at all. Never a synthesized figure. */
export type QueryOutcome =
  | { kind: "snapshot"; snapshot: AnalyticsSnapshot }
  | { kind: "refused"; message: string }
  | { kind: "unavailable" };

type AnalyticsPort = { query(query: unknown): unknown };

function isSnapshot(value: object): value is AnalyticsSnapshot {
  const candidate = value as Partial<AnalyticsSnapshot>;
  return typeof candidate.scope === "object" && typeof candidate.recording === "object" && typeof candidate.tokens === "object";
}

export function runQuery(query: unknown, analytics?: AnalyticsPort): QueryOutcome {
  if (!analytics) return { kind: "unavailable" };
  if (!query || typeof query !== "object" || Array.isArray(query)) {
    return { kind: "refused", message: "query must be an object" };
  }
  const answer = analytics.query(query);
  if (!answer || typeof answer !== "object") return { kind: "unavailable" };
  const refusal = answer as { error?: unknown; message?: unknown };
  if (typeof refusal.error === "string") {
    return { kind: "refused", message: typeof refusal.message === "string" ? refusal.message : refusal.error };
  }
  return isSnapshot(answer) ? { kind: "snapshot", snapshot: answer } : { kind: "unavailable" };
}

function* viewCall(method: string, args: unknown): HostIoSteps<QueryOutcome> {
  if (method !== "query") return { kind: "refused", message: `unknown method: ${method}` };
  const query = (args as { query?: unknown } | null)?.query;
  if (!query || typeof query !== "object" || Array.isArray(query)) return { kind: "refused", message: "query must be an object" };
  const answer = yield* ioValue<unknown>({op: "analyticsQuery", query: query as AnalyticsQuery});
  return runQuery(query, {query: () => answer});
}

const plugin: IoExports<PluginModule> = {
  viewCall,
};

export default defineProviderIo(plugin, () => host.io);
