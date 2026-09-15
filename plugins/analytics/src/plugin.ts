import type { AnalyticsSnapshot, HostAnalytics, PluginModule } from "@codeterm/plugin-sdk";

/** What the view receives: a snapshot, the host's refusal, or no answer at all. Never a synthesized figure. */
export type QueryOutcome =
  | { kind: "snapshot"; snapshot: AnalyticsSnapshot }
  | { kind: "refused"; message: string }
  | { kind: "unavailable" };

function hostAnalytics(): HostAnalytics | undefined {
  return typeof host === "object" && host ? host.analytics : undefined;
}

function isSnapshot(value: object): value is AnalyticsSnapshot {
  const candidate = value as Partial<AnalyticsSnapshot>;
  return typeof candidate.scope === "object" && typeof candidate.recording === "object" && typeof candidate.tokens === "object";
}

export function runQuery(query: unknown, analytics: HostAnalytics | undefined = hostAnalytics()): QueryOutcome {
  if (!analytics) return { kind: "unavailable" };
  if (!query || typeof query !== "object" || Array.isArray(query)) {
    return { kind: "refused", message: "query must be an object" };
  }
  const answer = analytics.query(query as Parameters<HostAnalytics["query"]>[0]) as unknown;
  if (!answer || typeof answer !== "object") return { kind: "unavailable" };
  const refusal = answer as { error?: unknown; message?: unknown };
  if (typeof refusal.error === "string") {
    return { kind: "refused", message: typeof refusal.message === "string" ? refusal.message : refusal.error };
  }
  return isSnapshot(answer) ? { kind: "snapshot", snapshot: answer } : { kind: "unavailable" };
}

function viewCall(method: string, args: unknown): QueryOutcome {
  if (method !== "query") return { kind: "refused", message: `unknown method: ${method}` };
  return runQuery((args as { query?: unknown } | null)?.query);
}

const plugin: PluginModule = {
  viewCall: viewCall as PluginModule["viewCall"],
};

export default plugin;
