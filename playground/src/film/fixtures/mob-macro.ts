import type { MemEdge, MemNode } from "../../../../src/components/MemoryPanel/types";

export const macroFact: MemNode = {
  id: "macro-short-replies", kind: "fact", title: "Keep replies short.",
  summary: "Lead with the answer.", body: "Use a sentence or two.",
  area: null, importance: 6, tags: [], has_vector: true, has_body: true,
};

export const macroNeighbors: MemNode[] = [
  { ...macroFact, id: "macro-person", kind: "peer", title: "Owner", body: "", summary: null },
  { ...macroFact, id: "macro-project", kind: "note", title: "Domios", body: "", summary: null },
];

export const macroEdges: MemEdge[] = macroNeighbors.map((node) => ({
  from: macroFact.id, to: node.id, rel: "about", confidence: 0.95, valid_from: null, valid_to: null,
}));

export const macroSearchNodes = new Map([
  macroFact,
  { ...macroFact, id: "macro-note", kind: "note", title: "Review the next cut." },
  { ...macroFact, id: "macro-learning", kind: "learning", title: "Leave room for captions." },
].map((node) => [node.id, node]));
