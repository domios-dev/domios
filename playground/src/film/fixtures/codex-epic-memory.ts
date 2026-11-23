import type { MemNode } from "../../../../src/components/MemoryPanel/types";

export const epicMemory = new Map<string, MemNode>([
  ["recall", "fact", "Ask the memory of a paired machine with --mesh", "Full and ReadOnly peers expose memory; AgentIsolated peers do not."],
  ["history", "rule", "Archived memory keeps its history", "Merged records remain archived and can be restored."],
  ["offline", "learning", "Keyword recall works without the embedder", "New records are saved; their vectors can be computed later."],
].map(([id, kind, title, summary]): [string, MemNode] => [id, {
  id, kind, title, summary, area: "domios", importance: 7, tags: ["mac-mini"], has_vector: true, has_body: true,
  created_at: "2026-10-23T09:00:00Z", updated_at: "2026-10-23T09:00:00Z", access_count: 3,
}]));
