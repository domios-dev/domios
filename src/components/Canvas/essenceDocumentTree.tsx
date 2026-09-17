import { ChevronDown, ChevronRight, FileText, Folder } from "lucide-react";
import { useState } from "react";
import type { EssenceMapNode, EssenceMapProject } from "./essenceMapModel";
import { projectName } from "./EssenceMapProjectView";

export interface EssenceDocumentEntry { path: string; node: EssenceMapNode; }
interface TreeFolder { name: string; folders: Map<string, TreeFolder>; documents: EssenceDocumentEntry[]; }
const emptyFolder = (name = ""): TreeFolder => ({ name, folders: new Map(), documents: [] });

export function essenceDocumentEntries(project: EssenceMapProject): EssenceDocumentEntry[] {
  const nodesByPath = new Map<string, EssenceMapNode>();
  for (const node of project.nodes) {
    const path = node.source.path.replaceAll("\\", "/").replace(/^\/+/, "");
    if (!path || (path !== "essences" && !path.startsWith("essences/")) || path.split("/").some((part) => part === "." || part === "..")) continue;
    if (!nodesByPath.has(path)) nodesByPath.set(path, node);
  }
  return (project.file.documents ?? []).flatMap((rawPath) => {
    const path = rawPath.replaceAll("\\", "/").replace(/^\/+/, "");
    if (!path || (path !== "essences" && !path.startsWith("essences/")) || path.split("/").some((part) => part === "." || part === "..")) return [];
    const node: EssenceMapNode = nodesByPath.get(path) ?? {
      kind: "node", id: `document:${path}`, type: "capability", label: path.split("/").pop() ?? path,
      vector: null, status: "stable", source: { path, anchor: "", sha256: "" },
    };
    return [{ path, node }];
  });
}

function buildTree(entries: EssenceDocumentEntry[]): TreeFolder {
  const root = emptyFolder();
  for (const entry of entries) {
    const parts = entry.path.split("/");
    const document = parts.pop() ?? entry.path;
    let current = root;
    for (const part of parts) {
      const next = current.folders.get(part) ?? emptyFolder(part);
      current.folders.set(part, next);
      current = next;
    }
    current.documents.push({ ...entry, path: document });
  }
  return root;
}

function FolderRow({ value, path, onSelect }: { value: TreeFolder; path: string; onSelect: (entry: EssenceDocumentEntry) => void }) {
  const [open, setOpen] = useState(true);
  const folders = [...value.folders.values()].sort((a, b) => a.name.localeCompare(b.name));
  const documents = [...value.documents].sort((a, b) => a.path.localeCompare(b.path));
  return <li>{value.name && <button type="button" className="flex w-full min-w-0 items-center gap-1 rounded px-2 py-1 text-left text-xs text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" aria-expanded={open} aria-label={`${open ? "Collapse" : "Expand"} ${value.name}`} onClick={() => setOpen((current) => !current)}>{open ? <ChevronDown size={13} aria-hidden="true" /> : <ChevronRight size={13} aria-hidden="true" />}<Folder size={13} aria-hidden="true" /><span className="truncate">{value.name}</span></button>}{open && <ul className="ml-3 border-l border-[var(--ct-border)] pl-2">{folders.map((child) => <FolderRow key={child.name} value={child} path={`${path}/${child.name}`} onSelect={onSelect} />)}{documents.map((entry) => <li key={entry.path}><button type="button" data-testid="essence-document" data-document-path={`${path}/${entry.path}`} className="flex w-full min-w-0 items-center gap-1 rounded px-2 py-1 text-left text-xs text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" aria-label={`Open ${path}/${entry.path}`} onClick={() => onSelect({ ...entry, path: `${path}/${entry.path}` })}><FileText size={13} aria-hidden="true" /><span className="truncate">{entry.path}</span></button></li>)}</ul>}</li>;
}

export function EssenceDocumentTree({ projects, onSelect }: { projects: EssenceMapProject[]; onSelect: (project: EssenceMapProject, entry: EssenceDocumentEntry) => void }) {
  return <div className="absolute inset-0 min-w-0 overflow-auto p-3 sm:p-5" data-testid="essence-document-tree" role="tree" aria-label="Essence documents">{projects.map((project) => { const entries = essenceDocumentEntries(project); const root = buildTree(entries); return <section key={project.file.project_path} className="mb-4 min-w-0 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-2" aria-labelledby={`essence-tree-project-${project.file.project_path}`}><h2 id={`essence-tree-project-${project.file.project_path}`} className="truncate px-2 py-1 text-xs font-semibold uppercase tracking-[.14em] text-[var(--ct-muted-fg)]" title={project.file.project_path}>{projectName(project.file.project_path)}</h2>{entries.length === 0 ? <p className="px-2 py-3 text-xs text-[var(--ct-muted-fg)]" data-testid="essence-document-tree-empty">No essence documents found.</p> : <ul>{[...root.folders.values()].sort((a, b) => a.name.localeCompare(b.name)).map((child) => <FolderRow key={child.name} value={child} path={child.name} onSelect={(entry) => onSelect(project, entry)} />)}</ul>}</section>; })}</div>;
}
