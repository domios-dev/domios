import { describe, expect, it } from "vitest";
import { essenceDocumentEntries } from "./essenceDocumentTree";
import type { EssenceMapProject } from "./essenceMapModel";

const project = (nodes: EssenceMapProject["nodes"]): EssenceMapProject => ({
  file: { project_path: "/repo", map_path: "/repo/essences/map.jsonl", status: "ready" },
  status: "valid", nodes, edges: [],
});

const node = (id: string, path: string): EssenceMapProject["nodes"][number] => ({
  kind: "node", id, type: "capability", label: id, vector: null, status: "stable",
  source: { path, anchor: id, sha256: "a".repeat(64) },
});

describe("essence_document_tree", () => {
  it("projects source-backed essence paths once and keeps document hierarchy inputs", () => {
    const entries = essenceDocumentEntries(project([
      node("a", "essences/CORE.md"), node("b", "essences/CORE.md"), node("c", "essences/contributing/style.md"),
    ]));
    expect(entries.map((entry) => entry.path)).toEqual(["essences/CORE.md", "essences/contributing/style.md"]);
    expect(entries[0].node.id).toBe("a");
  });

  it("rejects traversal and paths outside essences", () => {
    expect(essenceDocumentEntries(project([node("a", "../secret.md"), node("b", "/tmp/secret.md"), node("c", "notes.md")]))).toEqual([]);
  });
});
