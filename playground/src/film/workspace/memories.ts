import type { MemNode } from "../../../../src/components/MemoryPanel/types";
import { FILM_NOW } from "../fixtures/clock";

const records = [
  ["fact", "Offline notes keep their local revision"],
  ["fact", "The QA matrix runs on gpu-rig"],
  ["rule", "Attach test evidence before review"],
  ["note", "Reproduce reconnect order on laptop"],
  ["fact", "Sync resumes when the last device reconnects"],
  ["learning", "Clock skew exposed a stale timestamp"],
  ["note", "Release 2.4 verification checklist"],
  ["rule", "Keep independent fixes in separate lanes"],
  ["fact", "The guide ships with the sync release"],
  ["learning", "Delete versus edit needs three devices"],
  ["note", "Laptop repro and expected outcome"],
  ["rule", "Record concrete pass and fail evidence"],
  ["note", "Retry the matrix after the resolver changes"],
  ["fact", "Queued edits replay in order"],
  ["learning", "Airplane mode is the fastest offline repro"],
  ["note", "Archived release handoff"],
  ["fact", "A tombstone records a deleted note"],
  ["rule", "Check both machines before closing the task"],
] as const;

export const MEMORY_NODES: MemNode[] = records.map(([kind, title], index) => ({
  id: `sample-memory-${index}`, kind, title, summary: "Fieldnote release verification", body: title,
  area: "fieldnote", tags: ["offline-sync"], importance: 3 + (index % 5), has_vector: true, has_body: true,
  created_at: new Date(FILM_NOW).toISOString(), updated_at: new Date(FILM_NOW).toISOString(),
}));

export const MEMORY_EDGES = MEMORY_NODES.slice(1).map((node, index) => ({
  from: MEMORY_NODES[index === 0 ? 0 : Math.floor(index / 2)].id, to: node.id, rel: "relates_to", confidence: 0.9,
}));
