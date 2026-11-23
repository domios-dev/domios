import { useLayoutEffect, useRef, type ReactNode } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import { MemoryNodeDetail } from "../../../../../src/components/MemoryPanel/MemoryNodeDetail";
import { MemoryListTab } from "../../../../../src/components/MemoryPanel/MemoryListTab";
import { API } from "../../../gallery/fixtures";
import { lerp, range, StageProvider, useStage } from "../../engine";
import { macroEdges, macroFact, macroNeighbors, macroSearchNodes } from "../../fixtures/mob-macro";
import { NODES, nodeRadius, project, rankFrom, StageCanvas } from "../lab/labGraph";

const noop = () => undefined;
const rank = rankFrom(221);
const css = `
.mob-macro-write [data-testid="user-bubble-row"] { width:100%; }
.mob-macro-write .ct-user-bubble { width:100%; min-height:150px; padding:22px; display:flex; align-items:center; font-size:22px; line-height:1.5; }
.mob-macro-detail [data-testid="memory-node-detail"] { width:100%; padding:38px; gap:18px; border:0; font-size:12px; }
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(3),
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(5),
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(6),
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(n+8),
.mob-macro-detail [data-testid="memory-node-detail-tuck"] { display:none; }
.mob-macro-detail [data-testid="memory-node-detail"] > :first-child strong { font-size:20px; line-height:1.3; }
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(2) { font-size:12px; }
.mob-macro-detail [data-testid="memory-node-body-preview"] { font-family:inherit; font-size:14px; line-height:1.65; padding:14px; }
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(4) > :first-child,
.mob-macro-detail [data-testid="memory-node-detail"] > :nth-child(7) > :first-child { display:none; }
.mob-macro-detail [data-testid="memory-node-related-item"] { font-size:13px; padding:8px 0; }
.mob-macro-detail [data-testid="memory-node-related-item"] span { font-size:12px; }
.mob-macro-list [data-testid="memory-list-tab"] > :nth-child(2),
.mob-macro-list [data-testid="memory-list-sort"],
.mob-macro-list [data-testid="memory-list-item"] button,
.mob-macro-list [data-testid="memory-list-item"] .tabular-nums { display:none; }
.mob-macro-list [data-testid="memory-list-item"] span { font-size:12px; }
.mob-macro-list [data-testid="memory-list-item"] { padding:10px 6px; }
.mob-macro-list [data-testid="memory-list-item"] .font-medium { font-size:18px; }
.mob-macro-list [data-testid="memory-list-search"] { font-size:16px; padding-top:12px; padding-bottom:12px; }
`;

export function MacroFrame({ children, t }: { children: ReactNode; t: number }) {
  const { width, height } = useStage();
  const fit = Math.min(width / 1080, height / 1920);
  return <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
    <div style={{ position: "absolute", left: (width - 1080 * fit) / 2, top: (height - 1920 * fit) / 2, width: 1080, height: 1920, overflow: "hidden", transform: `scale(${fit})`, transformOrigin: "top left", color: "var(--ct-fg)" }}>
      <style>{css}</style>
      <StageProvider width={1080} height={1920}>{children}</StageProvider>
      <div style={{ position: "absolute", inset: 0, pointerEvents: "none", backdropFilter: "blur(16px)", maskImage: `radial-gradient(ellipse ${lerp(480, 510, range(t, 0, 6))}px 760px at 50% 43%, transparent 58%, var(--ct-bg) 112%)` }} />
      <div style={{ position: "absolute", inset: 0, pointerEvents: "none", background: "radial-gradient(ellipse at 50% 43%, transparent 36%, color-mix(in srgb, var(--ct-bg) 48%, transparent) 100%)" }} />
      <div style={{ position: "absolute", inset: 0, pointerEvents: "none", background: "linear-gradient(180deg, transparent 64%, var(--ct-bg) 77%)" }} />
    </div>
  </div>;
}

export function Write({ t }: { t: number }) {
  const { portrait } = useStage();
  const text = "Remember: keep replies short.";
  const content = text.slice(0, Math.max(1, Math.floor(text.length * range(t, 0, 3.6, (k) => k))));
  const k = range(t, 0, 6);
  const scale = portrait ? lerp(3.55, 3.85, k) : lerp(3.65, 3.95, k);
  return <MacroFrame t={t}>
    <div className="gallery-transcript mob-macro-write" style={{ position: "absolute", left: 540 - 248 * scale / 2, top: lerp(530, 475, k), width: 248, transform: `scale(${scale})`, transformOrigin: "top left" }}>
      <MessageBubble message={{ id: "macro-request", type: "user", content: `${content}${t < 3.7 ? "▏" : ""}` }} />
    </div>
  </MacroFrame>;
}

export function Saved({ t }: { t: number }) {
  const { portrait } = useStage();
  const k = range(t, 0, 6);
  const count = t < 2.5 ? 1 : 2;
  const scale = portrait ? lerp(2.6, 2.75, k) : lerp(3.65, 3.95, k);
  return <MacroFrame t={t}>
    <div className="mob-macro-detail" style={{ position: "absolute", left: portrait ? (1080 - 330 * scale) / 2 : lerp(-60, -100, k), top: portrait ? lerp(160, 100, k) : lerp(190, 120, k), width: 330, height: 500, overflow: "hidden", background: "var(--ct-surface)", border: "1px solid var(--ct-border)", borderRadius: 16, boxShadow: "var(--ct-shadow-xl)", transform: `scale(${scale})`, transformOrigin: "top left" }}>
      <MemoryNodeDetail api={API} node={macroFact} edges={macroEdges.slice(0, count)} nodes={new Map(macroNeighbors.map((n) => [n.id, n]))} onUpdate={noop} onDelete={noop} onSelectNode={noop} />
    </div>
  </MacroFrame>;
}

export function Graph({ t }: { t: number }) {
  const { portrait } = useStage();
  const k = range(t, 0, 6);
  const alpha = Float32Array.from(rank, (r) => r < 20 ? 1 : 0.3 + 0.7 * range(t, 0.5 + Math.log1p(r) * 0.24, 2 + Math.log1p(r) * 0.24));
  const [x, y] = NODES[221];
  const view = { cx: x + 12 * k, cy: y - 6 * k, scale: lerp(6.8, 7.8, k), fx: 540, fy: 790, rotate: lerp(-0.035, 0.035, k) };
  if (portrait) {
    Object.assign(view, { cx: 0, cy: 0, fx: 0, fy: 0, scale: lerp(0.36, 0.4, k) });
    let left = Infinity, right = -Infinity, top = Infinity, bottom = -Infinity;
    for (const node of NODES) {
      const [nx, ny] = project(view, node[0], node[1]);
      const radius = nodeRadius(node, view.scale);
      left = Math.min(left, nx - radius);
      right = Math.max(right, nx + radius);
      top = Math.min(top, ny - radius);
      bottom = Math.max(bottom, ny + radius);
    }
    view.fx = 540 - (left + right) / 2;
    view.fy = 790 - (top + bottom) / 2;
  }
  const [fx, fy] = project(view, x, y);
  return <MacroFrame t={t}>
    <StageCanvas t={t} layers={[{ view, nodeAlpha: alpha, edgeBoost: 1.1 }]} />
    <svg width={1080} height={1920} style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
      <defs><radialGradient id="mob-macro-focus"><stop stopColor="var(--ct-accent)" stopOpacity={0.35} /><stop offset="1" stopColor="var(--ct-accent)" stopOpacity={0} /></radialGradient></defs>
      <circle cx={fx} cy={fy} r={105} fill="url(#mob-macro-focus)" />
      <circle cx={fx} cy={fy} r={30 + 4 * k} fill="var(--ct-accent)" />
      <circle cx={fx} cy={fy} r={48 + 6 * k} fill="none" stroke="var(--ct-accent)" strokeWidth={1.5} opacity={0.6} />
    </svg>
  </MacroFrame>;
}

function useSearchFrame(query: string, t: number) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const input = ref.current?.querySelector<HTMLInputElement>("[data-testid='memory-list-search']");
    if (input) { input.value = query; input.placeholder = "Search memory…"; }
  }, [query, t]);
  return ref;
}

export function Recall({ t }: { t: number }) {
  const query = "short replies".slice(0, Math.floor(13 * range(t, 0, 2.6, (k) => k)));
  const ref = useSearchFrame(query, t);
  const k = range(t, 0, 6);
  const scale = lerp(3.5, 3.7, k);
  return <MacroFrame t={t}>
    <div ref={ref} className="mob-macro-list" style={{ position: "absolute", left: 540 - 260 * scale / 2, top: lerp(540, 485, k), width: 260, height: 156, overflow: "hidden", borderRadius: 14, border: "1px solid var(--ct-border)", background: "var(--ct-surface)", boxShadow: "var(--ct-shadow-xl)", transform: `scale(${scale})`, transformOrigin: "top left" }}>
      <MemoryListTab api={API} nodes={t < 2.7 ? macroSearchNodes : new Map([[macroFact.id, macroFact]])} selectedId={null} onSelectItem={noop} onViewInGraph={noop} updateNode={noop} removeNode={noop} />
    </div>
  </MacroFrame>;
}
