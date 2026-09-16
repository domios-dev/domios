import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "../../lib/types";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import { essenceDocumentEntries } from "./essenceDocumentTree";
import type { EssenceMapProject } from "./essenceMapModel";

const project = (nodes: EssenceMapProject["nodes"], documents = nodes.map((item) => item.source.path)): EssenceMapProject => ({
  file: { project_path: "/repo", map_path: "/repo/essences/map.jsonl", status: "ready", documents },
  status: "valid", nodes, edges: [],
});

const node = (id: string, path: string): EssenceMapProject["nodes"][number] => ({
  kind: "node", id, type: "capability", label: id, vector: null, status: "stable",
  source: { path, anchor: id, sha256: "a".repeat(64) },
});

describe("essence_document_tree", () => {
  it("essence_tree_source lists the on-disk set and keeps map context", () => {
    const entries = essenceDocumentEntries(project([
      node("a", "essences/CORE.md"), node("b", "essences/CORE.md"), node("c", "essences/contributing/style.md"),
    ], ["essences/CORE.md", "essences/contributing/style.md", "essences/build-machine/README.md"]));
    expect(entries.map((entry) => entry.path)).toEqual([
      "essences/CORE.md", "essences/contributing/style.md", "essences/build-machine/README.md",
    ]));
    expect(entries[0].node.id).toBe("a");
    expect(entries[2].node.id).toBe("document:essences/build-machine/README.md");
  });

  it("rejects traversal and paths outside essences", () => {
    expect(essenceDocumentEntries(project([node("a", "../secret.md"), node("b", "/tmp/secret.md"), node("c", "notes.md")]))).toEqual([]);
  });

  it("essence_tree_source selects an unreferenced document through the exact source panel", async () => {
    const source = { path: "essences/CORE.md", anchor: "capability:core", sha256: "a".repeat(64) };
    const map: EssenceMapProject = project([{ kind: "node", id: "capability:core", type: "capability", label: "Core", vector: null, status: "stable", source }], ["essences/CORE.md", "essences/contributing/unreferenced.md"]);
    const workspace: Workspace = { id: "ws", name: "Workspace", tabs: [], active_tab_id: null, cwd: "/repo" };
    const sourceReader = vi.fn(async () => "# exact core source");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<EssenceMapCanvasInner workspace={workspace} mapFiles={[{ ...map.file, content: JSON.stringify(map.nodes[0]) }]} sourceReader={sourceReader} />));
    expect(host.querySelector('[data-testid="essence-map-projects"]')).not.toBeNull();
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="essence-map-view-toggle"]')?.click());
    expect(host.querySelector('[data-testid="essence-document-tree"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="essence-map-projects"]')).toBeNull();
    expect(host.querySelector('[data-testid="essence-map-document-surface"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-testid="essence-document"]')).toHaveLength(2);
    const unreferenced = host.querySelector<HTMLButtonElement>('[data-document-path="essences/contributing/unreferenced.md"]');
    expect(unreferenced).not.toBeNull();
    await act(async () => unreferenced?.click());
    await act(async () => { await Promise.resolve(); });
    expect(sourceReader).toHaveBeenCalledWith("/repo", "essences/contributing/unreferenced.md");
    expect(host.querySelector('[data-testid="essence-source-panel"]')?.textContent).toContain("exact core source");
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="essence-map-view-toggle"]')?.click());
    expect(host.querySelector('[data-testid="essence-document-tree"]')).toBeNull();
    expect(host.querySelector('[data-testid="essence-map-projects"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="essence-map-document-surface"]')).toBeNull();
    await act(async () => root.unmount());
    host.remove();
  });
});

afterEach(() => { document.body.replaceChildren(); });
