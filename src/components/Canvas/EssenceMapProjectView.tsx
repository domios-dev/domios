import { useMemo, useState } from "react";
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
  // Fifty-five nodes and a hundred edges overwhelm at a glance; lighting one
  // neighbourhood at a time is what makes an essence and its connections readable.
  // Click pins a neighbourhood so it stays lit while the source panel is read;
  // hover is the transient preview on top of it.
  const [pinned, setPinned] = useState<string | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const active = pinned ?? hovered;
  const related = useMemo(() => {
    if (!active) return null;
    const set = new Set<string>([active]);
    for (const edge of project.edges) {
      if (edge.from === active) set.add(edge.to);
      if (edge.to === active) set.add(edge.from);
    }
    for (const item of layout.nodes) {
      if (item.node.vector === active) set.add(item.node.id);
      if (item.node.id === active && item.node.vector) set.add(item.node.vector);
    }
    return set;
  }, [active, project.edges, layout.nodes]);
  const nodeOpacity = (id: string): number => (related && !related.has(id) ? 0.12 : 1);
  const edgeOpacity = (base: number, from: string, to: string): number => (!active ? base : from === active || to === active ? Math.min(1, base * 2.4) : base * 0.1);
  return <div className="absolute" data-testid="essence-map-project" data-project-path={project.file.project_path} style={{ left: project.x, top: project.y, width: layout.width }} aria-labelledby={`essence-project-${project.file.project_path}`}>
    <div className="flex min-w-0 items-baseline gap-2 pb-1 pl-1"><h2 id={`essence-project-${project.file.project_path}`} className="truncate text-xs font-semibold uppercase tracking-[.18em] text-[var(--ct-muted-fg)]" title={project.file.project_path}>{projectName(project.file.project_path)}</h2><span className={`${UI_TEXT.metaMuted} shrink-0`}>{statusLabel(project)}</span></div>
    {project.status === "valid" && project.nodes.length > 0 ? <div className="relative max-w-full overflow-hidden" data-testid="essence-map-project-world" style={{ height: layout.height }}>
      <svg aria-hidden="true" className="pointer-events-none absolute inset-0 h-full w-full" viewBox={`0 0 ${layout.width} ${layout.height}`} preserveAspectRatio="none">
        {layout.nodes.filter((item) => item.node.type === "capability" && item.node.vector && nodeById.has(item.node.vector)).map((item) => <path key={`owns:${item.node.id}`} d={edgePath(nodeById.get(item.node.vector as string) as EssenceMapNodeLayout, item)} fill="none" stroke={colorByNodeId.get(item.node.id) ?? "var(--ct-border)"} strokeWidth="1.1" opacity={edgeOpacity(0.55, item.node.vector as string, item.node.id)} />)}
        {project.edges.map((edge) => { const source = nodeById.get(edge.from); const target = nodeById.get(edge.to); if (!source || !target) return null; const style = edge.type === "runs_on" ? { stroke: "var(--ct-muted-fg)", dash: "1 5", opacity: ".24", width: "1" } : edge.type === "client_of" ? { stroke: "var(--ct-fg)", dash: "9 5", opacity: ".3", width: "1.2" } : edge.type === "part_of" ? { stroke: "var(--ct-muted-fg)", dash: undefined, opacity: ".32", width: "1" } : { stroke: "var(--ct-fg)", dash: undefined, opacity: ".4", width: "1.1" }; return <path key={`${edge.type}:${edge.from}:${edge.to}`} data-testid="essence-map-edge" d={edgePath(source, target)} fill="none" stroke={style.stroke} strokeDasharray={style.dash} strokeWidth={style.width} opacity={edgeOpacity(Number(style.opacity), edge.from, edge.to)} />; })}
      </svg>
      {layout.nodes.map((item) => {
        const type = item.node.type;
        const color = colorByNodeId.get(item.node.id) ?? "var(--ct-muted-fg)";
        const hubNumber = type === "vector" ? (layout.clusters.find((cluster) => cluster.vectorId === item.node.id)?.colorIndex ?? 0) + 1 : null;
        const shape = type === "vector" ? { width: 30, height: 30, borderRadius: "9999px", background: color, border: "none" }
          : type === "capability" ? { width: 9, height: 9, borderRadius: "9999px", background: color, border: "none" }
          : type === "core" ? { width: 22, height: 22, borderRadius: 5, background: "var(--ct-surface)", border: `1.5px solid var(--ct-muted-fg)` }
          : { width: 30, height: 18, borderRadius: "9999px", background: "transparent", border: `1.5px solid var(--ct-fg)` };
        const below = type !== "capability";
        return <button key={item.node.id} type="button" data-testid="essence-map-node" data-node-id={item.node.id} data-node-type={type} data-node-vector={item.node.vector ?? undefined} aria-label={`${item.node.label} (${type})`} title={item.node.label} className="group absolute flex items-center justify-center transition-opacity focus-visible:z-10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" style={{ left: item.x, top: item.y, width: item.width, height: item.height, opacity: nodeOpacity(item.node.id), zIndex: related?.has(item.node.id) ? 5 : undefined }} onMouseEnter={() => setHovered(item.node.id)} onMouseLeave={() => setHovered(null)} onFocus={() => setHovered(item.node.id)} onBlur={() => setHovered(null)} onClick={() => { setPinned((current) => (current === item.node.id ? null : item.node.id)); onActivate(project, item.node); }}>
          <span aria-hidden="true" className="flex shrink-0 items-center justify-center transition-transform group-hover:scale-110" style={shape}>{hubNumber !== null && <span className="text-[13px] font-medium leading-none text-[var(--ct-bg)]">{hubNumber}</span>}</span>
          <span className={`pointer-events-none absolute ${below ? "left-1/2 top-full mt-1.5 -translate-x-1/2 text-center" : "left-full top-1/2 ml-2 -translate-y-1/2 whitespace-nowrap"} ${type === "vector" ? "w-[150px] text-[10px] font-semibold uppercase leading-[1.25] tracking-[.09em] text-[var(--ct-fg)]" : `leading-none text-[10px] text-[var(--ct-muted-fg)] group-hover:text-[var(--ct-fg)] ${below ? "w-[90px]" : ""}`}`} style={{ textShadow: "0 0 4px var(--ct-bg), 0 0 4px var(--ct-bg), 0 0 4px var(--ct-bg)" }}>{item.node.label}</span>
          <span className="sr-only">Source {item.node.source.path}, anchor {item.node.source.anchor}</span>
        </button>;
      })}
    </div> : <div className="flex min-h-24 items-center gap-3 px-4 py-5 text-xs text-[var(--ct-muted-fg)]" data-testid={`essence-map-project-${project.status}`}>{["malformed", "oversized", "unreadable"].includes(project.status) ? <AlertTriangle size={15} aria-hidden="true" className="shrink-0 text-[var(--ct-yellow)]" /> : <FileText size={15} aria-hidden="true" className="shrink-0" />}<span>{project.error ?? statusLabel(project)}</span></div>}
  </div>;
}
