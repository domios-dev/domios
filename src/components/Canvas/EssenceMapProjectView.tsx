import type { CSSProperties } from "react";
import { AlertTriangle, FileText } from "lucide-react";
import { UI_TEXT } from "../shared/design";
import type { EssenceMapNode, EssenceMapNodeLayout, EssenceMapProject, EssenceMapWorldProject } from "./essenceMapModel";

const COLORS = ["var(--ct-accent, #58a6ff)", "var(--ct-cyan, #39d0d8)", "var(--ct-green, #7ee787)", "var(--ct-yellow, #d29922)", "var(--ct-magenta, #d2a8ff)", "var(--ct-orange, #f0883e)", "var(--ct-red, #ff7b72)"] as const;

function vectorColor(index: number): string { return COLORS[index % COLORS.length]; }

function edgePath(source: EssenceMapNodeLayout, target: EssenceMapNodeLayout): string {
  const a = { x: source.x + source.width / 2, y: source.y + source.height / 2 };
  const b = { x: target.x + target.width / 2, y: target.y + target.height / 2 };
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const distance = Math.max(1, Math.hypot(dx, dy));
  const startScale = 0.5 / Math.max(Math.abs(dx) / source.width, Math.abs(dy) / source.height, 0.001);
  const endScale = 0.5 / Math.max(Math.abs(dx) / target.width, Math.abs(dy) / target.height, 0.001);
  const start = { x: a.x + dx * startScale, y: a.y + dy * startScale };
  const end = { x: b.x - dx * endScale, y: b.y - dy * endScale };
  const bend = Math.min(96, Math.max(24, distance * 0.16));
  if (Math.abs(dy) >= Math.abs(dx)) {
    const direction = dy >= 0 ? 1 : -1;
    return `M ${start.x} ${start.y} C ${start.x} ${start.y + direction * bend}, ${end.x} ${end.y - direction * bend}, ${end.x} ${end.y}`;
  }
  const direction = dx >= 0 ? 1 : -1;
  return `M ${start.x} ${start.y} C ${start.x + direction * bend} ${start.y}, ${end.x - direction * bend} ${end.y}, ${end.x} ${end.y}`;
}

function nodeStyle(node: EssenceMapNode, color: string): CSSProperties {
  if (node.type === "vector") return { borderColor: color, backgroundColor: `color-mix(in srgb, ${color} 16%, var(--ct-surface))`, boxShadow: `0 0 0 1px color-mix(in srgb, ${color} 18%, transparent)` };
  if (node.type === "capability") return { borderColor: `color-mix(in srgb, ${color} 58%, var(--ct-border))`, backgroundColor: `color-mix(in srgb, ${color} 7%, var(--ct-surface))` };
  const structural = node.type === "core" ? "var(--ct-accent, #58a6ff)" : "var(--ct-green, #7ee787)";
  return { borderColor: `color-mix(in srgb, ${structural} 52%, var(--ct-border))`, backgroundColor: `color-mix(in srgb, ${structural} 8%, var(--ct-surface))` };
}

function statusLabel(project: EssenceMapProject): string {
  if (project.status === "valid") return `${project.nodes.length} nodes · ${project.edges.length} links`;
  if (project.status === "empty") return "Empty essence map";
  if (project.status === "malformed") return "Malformed essence map";
  if (project.status === "oversized") return "Essence map is oversized";
  if (project.status === "missing") return "No essences/map.jsonl found";
  return "Essence map could not be read";
}

function projectName(path: string): string { return path.replaceAll("\\", "/").replace(/\/+$/, "").split("/").pop() || "Project"; }

export function EssenceMapProjectView({ project, onActivate }: { project: EssenceMapWorldProject; onActivate: (project: EssenceMapProject, node: EssenceMapNode) => void }) {
  const { layout } = project;
  const nodeById = layout.nodeById;
  const colorByNodeId = new Map(layout.clusters.flatMap((cluster) => cluster.nodeIds.map((id) => [id, vectorColor(cluster.colorIndex)] as const)));
  return <section className="ct-panel-card relative min-w-0 overflow-hidden" data-testid="essence-map-project" data-project-path={project.file.project_path} style={{ position: "absolute", left: project.x, top: project.y, width: layout.width }} aria-labelledby={`essence-project-${project.file.project_path}`}>
    <div className="flex min-w-0 items-start justify-between gap-3 border-b border-[var(--ct-border)] px-4 py-3"><div className="min-w-0"><h2 id={`essence-project-${project.file.project_path}`} className="truncate text-sm font-semibold text-[var(--ct-fg)]">{projectName(project.file.project_path)}</h2><p className={`${UI_TEXT.detailMuted} truncate`} title={project.file.project_path}>{project.file.project_path}</p></div><span className={`${UI_TEXT.metaMuted} shrink-0`}>{statusLabel(project)}</span></div>
    {project.status === "valid" && project.nodes.length > 0 ? <div className="relative max-w-full overflow-hidden" data-testid="essence-map-project-world" style={{ height: layout.height }}>
      <svg aria-hidden="true" className="pointer-events-none absolute inset-0 h-full w-full" viewBox={`0 0 ${layout.width} ${layout.height}`} preserveAspectRatio="none">
        {layout.clusters.map((cluster) => { const color = vectorColor(cluster.colorIndex); return <g key={`cluster:${cluster.vectorId ?? "unassigned"}`} data-testid="essence-map-vector-cluster" data-vector-id={cluster.vectorId ?? undefined}><rect x={cluster.x} y={cluster.y} width={cluster.width} height={cluster.height} rx="12" fill={`color-mix(in srgb, ${color} 4%, var(--ct-bg))`} stroke={`color-mix(in srgb, ${color} 25%, var(--ct-border))`} /><text x={cluster.x + 12} y={cluster.y + 17} fill="var(--ct-muted-fg)" fontSize="9" letterSpacing="1.4">{cluster.vector ? "VECTOR CLUSTER" : "UNASSIGNED CAPABILITIES"}</text></g>; })}
        {layout.regions.map((region) => <g key={`region:${region.type}`} data-testid={`essence-map-region-${region.type}`}><rect x={region.x} y={region.y} width={region.width} height={region.height} rx="12" fill="color-mix(in srgb, var(--ct-accent) 3%, var(--ct-bg))" stroke="var(--ct-border)" /><text x={region.x + 14} y={region.y + 18} fill="var(--ct-muted-fg)" fontSize="9" letterSpacing="1.4">{region.label.toUpperCase()}</text></g>)}
        {project.edges.map((edge) => { const source = nodeById.get(edge.from); const target = nodeById.get(edge.to); if (!source || !target) return null; const color = colorByNodeId.get(edge.from) ?? "var(--ct-border)"; const dashed = edge.type === "client_of" ? "3 4" : edge.type === "part_of" ? "5 5" : undefined; return <path key={`${edge.type}:${edge.from}:${edge.to}`} data-testid="essence-map-edge" d={edgePath(source, target)} fill="none" stroke={edge.type === "client_of" ? "var(--ct-green, #7ee787)" : edge.type === "part_of" ? "var(--ct-muted-fg)" : color} strokeDasharray={dashed} strokeWidth="1.35" opacity=".55" />; })}
      </svg>
      {layout.nodes.map((item) => <button key={item.node.id} type="button" data-testid="essence-map-node" data-node-id={item.node.id} data-node-type={item.node.type} data-node-vector={item.node.vector ?? undefined} aria-label={`${item.node.label} (${item.node.type})`} title={item.node.label} className="absolute overflow-hidden rounded-md border bg-[var(--ct-surface)] px-3 py-2 text-left shadow-sm transition-colors hover:border-[var(--ct-accent)] hover:bg-[var(--ct-hover)] focus-visible:z-10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" style={{ left: item.x, top: item.y, width: item.width, height: item.height, ...nodeStyle(item.node, colorByNodeId.get(item.node.id) ?? "var(--ct-border)") }} onClick={() => onActivate(project, item.node)}><span className="block text-[9px] uppercase tracking-[.16em] text-[var(--ct-muted)]">{item.node.type}</span><span className="mt-1 block max-h-12 overflow-hidden text-[11px] font-medium leading-4 text-[var(--ct-fg)]">{item.node.label}</span><span className="sr-only">Source {item.node.source.path}, anchor {item.node.source.anchor}</span></button>)}
    </div> : <div className="flex min-h-24 items-center gap-3 px-4 py-5 text-xs text-[var(--ct-muted-fg)]" data-testid={`essence-map-project-${project.status}`}>{["malformed", "oversized", "unreadable"].includes(project.status) ? <AlertTriangle size={15} aria-hidden="true" className="shrink-0 text-[var(--ct-yellow)]" /> : <FileText size={15} aria-hidden="true" className="shrink-0" />}<span>{project.error ?? statusLabel(project)}</span></div>}
  </section>;
}
