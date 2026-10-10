export interface PathTreeLeaf<T> { id: string; name: string; item: T }
export interface PathTreeFolder<T> { id: string; name: string; children: PathTreeNode<T>[] }
export type PathTreeNode<T> = PathTreeLeaf<T> | PathTreeFolder<T>;
export interface PathTreeRow<T> { node: PathTreeNode<T>; level: number }

export function isFolder<T>(node: PathTreeNode<T>): node is PathTreeFolder<T> {
  return "children" in node;
}

function normalize(path: string): string {
  return path.replaceAll("\\", "/");
}

function within(path: string, root: string | null | undefined): string | null {
  const base = root ? normalize(root).replace(/\/$/, "") : "";
  if (!base) return null;
  const caseless = /^[a-z]:/i.test(path);
  const compare = (value: string) => caseless ? value.toLowerCase() : value;
  if (compare(path) === compare(base)) return "";
  return compare(path).startsWith(`${compare(base)}/`) ? path.slice(base.length + 1) : null;
}

/** Replaces the home prefix with `~`; other paths keep forward slashes. */
export function shortenPath(path: string, home: string | null | undefined): string {
  const normalized = normalize(path);
  const rest = within(normalized, home);
  return rest === null ? normalized : rest ? `~/${rest}` : "~";
}

/** Relative to the working directory when inside it, otherwise home-shortened. */
export function displayPath(path: string, { cwd, home }: { cwd?: string | null; home?: string | null }): string {
  const normalized = normalize(path);
  if (!/^(?:[A-Za-z]:\/|\/|~\/)/.test(normalized)) return normalized;
  const relative = within(normalized, cwd);
  return relative ? relative : shortenPath(normalized, home);
}

export function pathName(path: string): string {
  return normalize(path).split("/").filter(Boolean).pop() ?? path;
}

/** Folders first, single-child folder chains compacted into one row. */
export function buildPathTree<T>(entries: readonly { id: string; path: string; item: T }[]): PathTreeNode<T>[] {
  const root: PathTreeNode<T>[] = [];
  for (const entry of entries) {
    const path = normalize(entry.path);
    const parts = path.split("/").filter(Boolean);
    if (path.startsWith("/") && parts.length) parts[0] = `/${parts[0]}`;
    let children = root;
    let id = "";
    for (const part of parts.slice(0, -1)) {
      id += `${part}/`;
      let folder = children.find((node): node is PathTreeFolder<T> => node.id === id && isFolder(node));
      if (!folder) { folder = { id, name: part, children: [] }; children.push(folder); }
      children = folder.children;
    }
    children.push({ id: entry.id, name: parts[parts.length - 1] ?? path, item: entry.item });
  }
  function compact(nodes: PathTreeNode<T>[]): PathTreeNode<T>[] {
    return nodes.map((node) => {
      if (!isFolder(node)) return node;
      let folder = node;
      while (folder.children.length === 1 && isFolder(folder.children[0])) {
        const child = folder.children[0];
        folder = { id: child.id, name: `${folder.name}/${child.name}`, children: child.children };
      }
      return { ...folder, children: compact(folder.children) };
    }).sort((a, b) => Number(isFolder(b)) - Number(isFolder(a)) || a.name.localeCompare(b.name));
  }
  return compact(root);
}

export function visibleTreeRows<T>(nodes: readonly PathTreeNode<T>[], collapsed: ReadonlySet<string>, level = 1): PathTreeRow<T>[] {
  return nodes.flatMap((node) => [{ node, level }, ...(isFolder(node) && !collapsed.has(node.id) ? visibleTreeRows(node.children, collapsed, level + 1) : [])]);
}

export function treeLeaves<T>(node: PathTreeNode<T>): T[] {
  return isFolder(node) ? node.children.flatMap(treeLeaves) : [node.item];
}

export function treeFocusIndex(code: string, index: number, count: number): number {
  if (code === "Home") return 0;
  if (code === "End") return count - 1;
  return Math.max(0, Math.min(count - 1, index + (code === "ArrowDown" ? 1 : code === "ArrowUp" ? -1 : 0)));
}

/** Index of the nearest row above `index` with a smaller level, or -1. */
export function treeParentIndex(rows: readonly { level: number }[], index: number): number {
  const level = rows[index]?.level ?? 0;
  for (let cursor = index - 1; cursor >= 0; cursor -= 1) if (rows[cursor].level < level) return cursor;
  return -1;
}

/** Breadcrumb segments; long paths keep the first segment and the last `tail` ones around an ellipsis. */
export function breadcrumbSegments(path: string, tail = 3): string[] {
  const parts = normalize(path).split("/").filter(Boolean);
  if (normalize(path).startsWith("/") && parts.length) parts[0] = `/${parts[0]}`;
  return parts.length <= tail + 1 ? parts : [parts[0], "…", ...parts.slice(-tail)];
}
