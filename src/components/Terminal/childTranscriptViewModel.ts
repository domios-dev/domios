import type { ChildMetadata } from "../../lib/childTranscript";

export function childTranscriptViewState({ loading, error, itemCount }: { loading: boolean; error: string | null; itemCount: number }): "loading" | "empty" | "error" | "ready" {
  if (itemCount > 0) return "ready";
  if (error) return "error";
  return loading ? "loading" : "empty";
}

export function childTranscriptTitle(child: ChildMetadata | null): string {
  return child ? `Subagent ${child.ordinal}` : "Subagent";
}

export function subagentTone(status: string | undefined): string {
  return status === "running" || status === "started" ? "active" : "scheduled";
}

export function compactSubagents<T>(items: readonly T[], expanded: boolean) {
  return { visible: expanded ? items : items.slice(0, 3), hidden: expanded ? 0 : Math.max(0, items.length - 3) };
}
