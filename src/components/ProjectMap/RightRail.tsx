import { useMemo, useState } from "react";
import { ChevronsLeft, ChevronsRight, Sparkles } from "lucide-react";
import { colorForKind } from "./styles";
import type { ProjectMapEdge, ProjectMapNode } from "./types";

interface Props {
  detail: {
    node: ProjectMapNode;
    incoming: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
    outgoing: Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>;
  } | null;
  onJump: (id: number) => void;
  onAskLlm?: (node: ProjectMapNode) => void;
}

export function RightRail({ detail, onJump, onAskLlm }: Props) {
  const [collapsed, setCollapsed] = useState(false);

  if (collapsed) {
    return (
      <div className="w-7 shrink-0 border-l border-[var(--ct-border)] bg-[var(--ct-bg)] flex flex-col items-center py-2">
        <button
          onClick={() => setCollapsed(false)}
          className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] p-1"
          title="Expand right rail"
        >
          <ChevronsLeft size={12} />
        </button>
      </div>
    );
  }

  return (
    <div className="w-72 shrink-0 border-l border-[var(--ct-border)] bg-[var(--ct-bg)] flex flex-col text-[11px] text-[var(--ct-fg)]">
      <div className="flex items-center justify-between px-2 py-1.5 border-b border-[var(--ct-border)]">
        <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
          Inspector
        </span>
        <button
          onClick={() => setCollapsed(true)}
          className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] p-0.5"
          title="Collapse"
        >
          <ChevronsRight size={12} />
        </button>
      </div>

      {detail == null && (
        <div className="px-3 py-4 text-[var(--ct-muted)] italic">
          Click a node to inspect.
        </div>
      )}

      {detail && <NodeBody detail={detail} onJump={onJump} onAskLlm={onAskLlm} />}
    </div>
  );
}

function NodeBody({
  detail,
  onJump,
  onAskLlm,
}: {
  detail: NonNullable<Props["detail"]>;
  onJump: (id: number) => void;
  onAskLlm?: (node: ProjectMapNode) => void;
}) {
  const { node, incoming, outgoing } = detail;
  const groupedIn = useMemo(() => groupByKind(incoming), [incoming]);
  const groupedOut = useMemo(() => groupByKind(outgoing), [outgoing]);

  const sigText = node.signature && node.signature.length > 0 ? node.signature : node.serialized_name;
  const pathLine = node.path
    ? node.start_line != null
      ? `${node.path}:${node.start_line + 1}`
      : node.path
    : null;

  return (
    <div className="flex-1 min-h-0 overflow-y-auto">
      <div className="px-3 py-2 border-b border-[var(--ct-border)]">
        <div className="flex items-center gap-2 mb-1">
          <span
            className="inline-block w-2 h-2 rounded-full"
            style={{ background: colorForKind(node.kind) }}
          />
          <span className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)]">
            {node.kind}
          </span>
        </div>
        <div className="font-mono text-[12px] text-[var(--ct-fg)] break-words leading-snug">
          {node.display_name}
        </div>
        {pathLine && (
          <div className="mt-1 font-mono text-[10px] text-[var(--ct-muted-fg)] break-all">
            {pathLine}
          </div>
        )}
      </div>

      <Section title="Signature">
        <div className="font-mono text-[10px] text-[var(--ct-muted-fg)] whitespace-pre-wrap break-words">
          {sigText}
        </div>
      </Section>

      <Section title={`Incoming (${incoming.length})`}>
        <EdgeList groups={groupedIn} onJump={onJump} direction="in" />
      </Section>

      <Section title={`Outgoing (${outgoing.length})`}>
        <EdgeList groups={groupedOut} onJump={onJump} direction="out" />
      </Section>

      <div className="px-3 py-2 border-t border-[var(--ct-border)]">
        <button
          onClick={() => onAskLlm?.(node)}
          className="w-full flex items-center justify-center gap-1 px-2 py-1.5 rounded-sm border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] transition-colors"
          title="Ask the LLM about this node"
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
      <div className="text-[10px] uppercase tracking-wide text-[var(--ct-muted)] mb-1">
        {title}
      </div>
      {children}
    </div>
  );
}

function EdgeList({
  groups,
  onJump,
  direction,
}: {
  groups: Map<string, Array<{ edge: ProjectMapEdge; other: ProjectMapNode }>>;
  onJump: (id: number) => void;
  direction: "in" | "out";
}) {
  if (groups.size === 0) {
    return <div className="text-[10px] text-[var(--ct-muted)] italic">none</div>;
  }
  return (
    <div className="flex flex-col gap-1">
      {Array.from(groups.entries()).map(([kind, items]) => (
        <div key={kind} className="flex flex-col gap-0.5">
          <div className="text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">
            {kind} ({items.length})
          </div>
          {items.slice(0, 16).map(({ other, edge }, idx) => (
            <button
              key={`${edge.source}-${edge.target}-${idx}`}
              onClick={() => onJump(other.id)}
              className="text-left px-1 py-0.5 rounded-sm hover:bg-[var(--ct-hover)] flex items-center gap-1 truncate"
              title={other.serialized_name}
            >
              <span className="text-[var(--ct-muted)] text-[9px]">
                {direction === "in" ? "←" : "→"}
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
          {items.length > 16 && (
            <span className="text-[9px] text-[var(--ct-muted)] italic px-1">
              +{items.length - 16} more…
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

function groupByKind<T extends { edge: ProjectMapEdge }>(items: T[]): Map<string, T[]> {
  const out = new Map<string, T[]>();
  for (const it of items) {
    const arr = out.get(it.edge.kind) ?? [];
    arr.push(it);
    out.set(it.edge.kind, arr);
  }
  return out;
}
