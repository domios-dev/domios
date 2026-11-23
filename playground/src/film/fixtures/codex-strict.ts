import type { MemNode } from "../../../../src/components/MemoryPanel/types";
import { FILM_NOW } from "./clock";

const titles = ["Ship with evidence", "Keep source context", "Review before merge"];

const memories: MemNode[] = titles.map((title, i) => ({
  id: `strict-memory-${i}`,
  title,
  kind: "note",
  summary: null,
  area: "project",
  importance: 6,
  tags: [],
  has_vector: true,
  has_body: true,
  created_at: new Date(FILM_NOW - 3_600_000).toISOString(),
  updated_at: new Date(FILM_NOW - i * 60_000).toISOString(),
  access_count: 1,
}));

export const STRICT_MEMORY_STATES = [1, 2, 3].map((count) => new Map(memories.slice(0, count).map((node) => [node.id, node])));
