import { useState, type KeyboardEvent, type ReactNode } from "react";
import { ChevronDown, ChevronRight, FileCode, FileText, Folder, Image } from "lucide-react";
import { FileTreeRow } from "../shared/FileTreeRow";
import type { ChatDocument } from "./chatDocuments";
import { buildDocumentTree, documentTreeFocusIndex, visibleDocumentRows, type DocumentTreeNode } from "./documentTree";

export function DocumentsTree({ documents, home, selected, onSelect, onFullscreen, renderActions }: {
  documents: readonly ChatDocument[];
  home: string | null;
  selected?: string;
  onSelect: (path: string) => void;
  onFullscreen: (path: string) => void;
  renderActions: (document: ChatDocument) => ReactNode;
}) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [focused, setFocused] = useState<string | null>(null);
  const rows = visibleDocumentRows(buildDocumentTree(documents, home), collapsed);
  const focusId = rows.some(({ node }) => node.id === focused) ? focused : rows.find(({ node }) => node.id === selected)?.node.id ?? rows[0]?.node.id;
  function toggle(id: string) {
    setCollapsed((current) => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; });
  }
  function activate(node: DocumentTreeNode) { if ("children" in node) toggle(node.id); else onSelect(node.document.path); }
  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const { node, level } = rows[index];
    if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.code)) {
      event.preventDefault();
      const next = documentTreeFocusIndex(event.code, index, rows.length);
      event.currentTarget.closest('[role="tree"]')?.querySelectorAll<HTMLButtonElement>('[role="treeitem"]')[next]?.focus();
    } else if (event.code === "Enter" || event.code === "Space") {
      event.preventDefault();
      if (event.code === "Space" && "document" in node) onFullscreen(node.document.path);
      else activate(node);
    } else if (event.code === "ArrowRight" && "children" in node) {
      event.preventDefault();
      if (collapsed.has(node.id)) toggle(node.id);
      else event.currentTarget.closest('[role="tree"]')?.querySelectorAll<HTMLButtonElement>('[role="treeitem"]')[index + 1]?.focus();
    } else if (event.code === "ArrowLeft") {
      event.preventDefault();
      if ("children" in node && !collapsed.has(node.id)) toggle(node.id);
      else {
        const parent = rows.slice(0, index).map((row) => row.level < level).lastIndexOf(true);
        if (parent >= 0) event.currentTarget.closest('[role="tree"]')?.querySelectorAll<HTMLButtonElement>('[role="treeitem"]')[parent]?.focus();
      }
    }
  }
  return <nav aria-label="Tab documents" className="mb-3 max-h-64 overflow-auto">
    <div role="tree" aria-label="Documents">
      {rows.map(({ node, level }, index) => {
        const folder = "children" in node;
        const Icon = folder ? Folder : node.document.kind === "image" ? Image : node.document.kind === "page" ? FileCode : FileText;
        const title = folder ? node.name : `${node.document.path}${node.document.shownAt == null ? "" : `\nLast shown: ${new Date(node.document.shownAt).toLocaleString()}`}`;
        return <div key={node.id} style={{ paddingLeft: (level - 1) * 12 }}>
          <FileTreeRow role="treeitem" aria-label={folder ? node.name : `Open ${node.name}`} aria-level={level} aria-expanded={folder ? !collapsed.has(node.id) : undefined} aria-selected={folder ? undefined : selected === node.id} selected={!folder && selected === node.id} tabIndex={focusId === node.id ? 0 : -1} title={title} onFocus={() => setFocused(node.id)} onClick={() => activate(node)} onKeyDown={(event) => onKeyDown(event, index)} actions={folder ? undefined : renderActions(node.document)}>
            {folder ? collapsed.has(node.id) ? <ChevronRight size={13} aria-hidden /> : <ChevronDown size={13} aria-hidden /> : <span className="w-[13px] shrink-0" />}
            <Icon size={13} aria-hidden className="shrink-0" /><span className="truncate">{node.name}</span>
          </FileTreeRow>
        </div>;
      })}
    </div>
  </nav>;
}

