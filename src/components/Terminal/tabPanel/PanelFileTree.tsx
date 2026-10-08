import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { ChevronDown, ChevronRight, Folder } from "lucide-react";
import { FileTreeRow } from "../../shared/FileTreeRow";
import { isFolder, treeFocusIndex, treeParentIndex, visibleTreeRows, type PathTreeFolder, type PathTreeLeaf, type PathTreeNode } from "./pathTree";

export interface PanelLeafView {
  label: string;
  icon: ReactNode;
  name?: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
  title?: string;
}

export function PanelFileTree<T>({ label, nodes, current, onOpen, leaf, folderMeta }: {
  label: string;
  nodes: readonly PathTreeNode<T>[];
  /** The item last opened: highlighted and focused when the tree comes back. */
  current?: string | null;
  onOpen: (node: PathTreeLeaf<T>) => void;
  leaf: (node: PathTreeLeaf<T>) => PanelLeafView;
  folderMeta?: (node: PathTreeFolder<T>) => ReactNode;
}) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [focused, setFocused] = useState<string | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  const rows = visibleTreeRows(nodes, collapsed);
  const focusId = rows.some(({ node }) => node.id === focused) ? focused : rows.find(({ node }) => node.id === current)?.node.id ?? rows[0]?.node.id;
  const items = () => treeRef.current?.querySelectorAll<HTMLButtonElement>('[role="treeitem"]');
  useEffect(() => {
    if (!current) return;
    const index = rows.findIndex(({ node }) => node.id === current);
    if (index >= 0) items()?.[index]?.focus({ preventScroll: false });
  }, []);
  function toggle(id: string) {
    setCollapsed((previous) => { const next = new Set(previous); if (next.has(id)) next.delete(id); else next.add(id); return next; });
  }
  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const { node } = rows[index];
    const folder = isFolder(node);
    if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.code)) {
      event.preventDefault();
      items()?.[treeFocusIndex(event.code, index, rows.length)]?.focus();
    } else if (event.code === "Enter" || event.code === "Space") {
      event.preventDefault();
      if (folder) toggle(node.id); else onOpen(node);
    } else if (event.code === "ArrowRight" && folder) {
      event.preventDefault();
      if (collapsed.has(node.id)) toggle(node.id); else items()?.[index + 1]?.focus();
    } else if (event.code === "ArrowLeft") {
      event.preventDefault();
      if (folder && !collapsed.has(node.id)) toggle(node.id);
      else items()?.[treeParentIndex(rows, index)]?.focus();
    }
  }
  return <div ref={treeRef} role="tree" aria-label={label}>
    {rows.map(({ node, level }, index) => {
      const folder = isFolder(node);
      const view = folder ? null : leaf(node);
      const open = folder && !collapsed.has(node.id);
      return <div key={node.id} style={{ paddingLeft: (level - 1) * 14 }}>
        <FileTreeRow role="treeitem" aria-level={level} aria-expanded={folder ? open : undefined}
          aria-label={folder ? node.name : view?.label} aria-selected={folder ? undefined : node.id === current}
          selected={!folder && node.id === current} tabIndex={focusId === node.id ? 0 : -1} title={folder ? node.name : view?.title}
          onFocus={() => setFocused(node.id)} onClick={() => folder ? toggle(node.id) : onOpen(node)} onKeyDown={(event) => onKeyDown(event, index)}
          actions={view?.actions}>
          {folder ? open ? <ChevronDown size={13} aria-hidden className="shrink-0 opacity-70" /> : <ChevronRight size={13} aria-hidden className="shrink-0 opacity-70" /> : <span className="w-[13px] shrink-0" />}
          {folder ? <Folder size={13} aria-hidden className="shrink-0 opacity-70" /> : view?.icon}
          <span className={`truncate ${folder ? "font-mono text-[11px]" : ""}`}>{folder ? node.name : view?.name ?? node.name}</span>
          {folder ? folderMeta && <span className="tab-panel__row-meta">{folderMeta(node)}</span> : view?.meta && <span className="tab-panel__row-meta">{view.meta}</span>}
        </FileTreeRow>
      </div>;
    })}
  </div>;
}
