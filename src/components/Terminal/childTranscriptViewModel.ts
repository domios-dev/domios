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
