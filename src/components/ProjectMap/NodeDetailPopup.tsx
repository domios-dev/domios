import { useEffect, useMemo } from "react";
import { X, Sparkles, ArrowRight } from "lucide-react";
import { colorForKind } from "./styles";
import type { ProjectMapEdge, ProjectMapNode } from "./types";

interface Props {
  detail: {
    node: ProjectMapNode;
    incoming: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
    outgoing: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
  } | null;
  onClose: () => void;
  onJump: (id: number) => void;
  onAskLlm?: (node: ProjectMapNode) => void;
}

export function NodeDetailPopup({ detail, onClose, onJump, onAskLlm }: Props) {
  useEffect(() => {
    if (detail == null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [detail, onClose]);

  const sigText = useMemo(() => {
    if (!detail) return "";
    const n = detail.node;
    return n.signature && n.signature.length > 0 ? n.signature : n.serialized_name;
  }, [detail]);

  if (!detail) return null;
  const { node, incoming, outgoing } = detail;
  const pathLine = node.path
    ? node.start_line != null
      ? `${node.path}:${node.start_line + 1}`
      : node.path
    : null;

  return (
    <div className="absolute top-3 right-3 z-40 w-[360px] max-h-[calc(100%-1.5rem)] flex flex-col rounded-sm border border-[var(--ct-border)] bg-[var(--ct-bg)] shadow-lg">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--ct-border)]">
        <span
          className="inline-block w-2 h-2 rounded-full shrink-0"
          style={{ background: colorForKind(node.kind) }}
        />
        <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)] shrink-0">
          {node.kind}
        </span>
        <span className="font-mono text-[12px] text-[var(--ct-fg)] flex-1 truncate" title={node.display_name}>
          {node.display_name}
        </span>
        <button
          onClick={onClose}
          className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] p-0.5"
          title="Close (Esc)"
        >
          <X size={12} />
        </button>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto text-[11px]">
        {pathLine && (
          <div className="px-3 py-2 border-b border-[var(--ct-border)] font-mono text-[10px] text-[var(--ct-muted-fg)] break-all">
            {pathLine}
          </div>
        )}
        {sigText && (
          <Section title="Signature">
            <div className="font-mono text-[10px] text-[var(--ct-muted-fg)] whitespace-pre-wrap break-words">
              {sigText}
            </div>
          </Section>
        )}
        <Section title={`Incoming (${incoming.length})`}>
          <EdgeList items={incoming} onJump={onJump} direction="in" />
        </Section>
        <Section title={`Outgoing (${outgoing.length})`}>
          <EdgeList items={outgoing} onJump={onJump} direction="out" />
        </Section>
      </div>
      <div className="px-3 py-2 border-t border-[var(--ct-border)] flex gap-2">
        <button
          onClick={() => onAskLlm?.(node)}
          className="flex-1 flex items-center justify-center gap-1 px-2 py-1.5 rounded-sm border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors text-[11px]"
        >
          <Sparkles size={11} />
          <span>Ask LLM</span>
        </button>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="px-3 py-2 border-b border-[var(--ct-border)]">
      <div className="text-[9px] uppercase tracking-wide text-[var(--ct-muted)] mb-1">
        {title}
      </div>
      {children}
    </div>
  );
}

function EdgeList({
  items,
  onJump,
  direction,
}: {
  items: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
  onJump: (id: number) => void;
  direction: "in" | "out";
}) {
  if (items.length === 0) {
    return <div className="text-[10px] text-[var(--ct-muted)] italic">none</div>;
  }
  const groups = new Map<string, typeof items>();
  for (const it of items) {
    const arr = groups.get(it.edge.kind) ?? [];
    arr.push(it);
    groups.set(it.edge.kind, arr);
  }
  return (
    <div className="flex flex-col gap-1">
      {Array.from(groups.entries()).map(([kind, list]) => (
        <div key={kind} className="flex flex-col gap-0.5">
          <div className="text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">
            {kind} ({list.length})
          </div>
          {list.slice(0, 12).map(({ other, edge }, idx) => (
            <button
              key={`${edge.source}-${edge.target}-${idx}`}
              onClick={() => onJump(other.id)}
              className="text-left px-1 py-0.5 rounded-sm hover:bg-[var(--ct-hover)] flex items-center gap-1 truncate"
              title={other.serialized_name}
            >
              <span className="text-[var(--ct-muted)] text-[9px]">
                {direction === "in" ? "←" : <ArrowRight size={9} />}
              </span>
              <span
                className="inline-block w-1.5 h-1.5 rounded-full shrink-0"
                style={{ background: colorForKind(other.kind) }}
              />
              <span className="font-mono truncate text-[var(--ct-fg)]">
                {other.display_name}
              </span>
            </button>
          ))}
          {list.length > 12 && (
            <span className="text-[9px] text-[var(--ct-muted)] italic px-1">
              +{list.length - 12} more…
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
