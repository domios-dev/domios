import type { ChatDocument } from "./chatDocuments";

export type DocumentTreeNode =
  | { id: string; name: string; document: ChatDocument }
  | { id: string; name: string; children: DocumentTreeNode[] };

export function shortenDocumentPath(path: string, home: string | null): string {
  const normalized = path.replaceAll("\\", "/");
  const root = home?.replaceAll("\\", "/").replace(/\/$/, "");
  const compare = (value: string) => /^[a-z]:/i.test(normalized) ? value.toLowerCase() : value;
  return root && (compare(normalized) === compare(root) || compare(normalized).startsWith(`${compare(root)}/`))
    ? `~${normalized.slice(root.length)}` : normalized;
}

export function buildDocumentTree(documents: readonly ChatDocument[], home: string | null): DocumentTreeNode[] {
  const root: DocumentTreeNode[] = [];
  for (const document of documents) {
    const path = shortenDocumentPath(document.path, home);
    const parts = path.split("/").filter(Boolean);
    if (path.startsWith("/")) parts[0] = `/${parts[0]}`;
    let children = root;
    let id = "";
    for (const part of parts.slice(0, -1)) {
      id += `${part}/`;
      let folder = children.find((node) => node.id === id && "children" in node);
      if (!folder) { folder = { id, name: part, children: [] }; children.push(folder); }
      if ("children" in folder) children = folder.children;
    }
    children.push({ id: document.path, name: document.name, document });
  }
  function compact(nodes: DocumentTreeNode[]): DocumentTreeNode[] {
    return nodes.map((node) => {
      if (!("children" in node)) return node;
      let folder = node;
      while (folder.children.length === 1 && "children" in folder.children[0]) {
        const child = folder.children[0];
        folder = { id: child.id, name: `${folder.name}/${child.name}`, children: child.children };
      }
      return { ...folder, children: compact(folder.children) };
    }).sort((a, b) => Number("children" in b) - Number("children" in a) || a.name.localeCompare(b.name));
  }
  return compact(root);
}

export function visibleDocumentRows(nodes: readonly DocumentTreeNode[], collapsed: ReadonlySet<string>, level = 1): { node: DocumentTreeNode; level: number }[] {
  return nodes.flatMap((node) => [{ node, level }, ...("children" in node && !collapsed.has(node.id) ? visibleDocumentRows(node.children, collapsed, level + 1) : [])]);
}

export function documentTreeFocusIndex(code: string, index: number, count: number): number {
  if (code === "Home") return 0;
  if (code === "End") return count - 1;
  return Math.max(0, Math.min(count - 1, index + (code === "ArrowDown" ? 1 : code === "ArrowUp" ? -1 : 0)));
}
