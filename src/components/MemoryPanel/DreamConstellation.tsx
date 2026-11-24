import { dreamRunData, type DreamWorker } from "./dreamModel";

function hash(text: string) {
  let n = 2166136261;
  for (const char of text) n = Math.imul(n ^ char.charCodeAt(0), 16777619);
  return n >>> 0;
}

export function constellationNodes(worker: DreamWorker, seed: string, cap: number) {
  const data = dreamRunData(worker);
  const degree = new Map<string, number>();
  for (const edge of data.edges) {
    degree.set(edge.from, (degree.get(edge.from) ?? 0) + 1);
    degree.set(edge.to, (degree.get(edge.to) ?? 0) + 1);
  }
  const ids = [...data.ids].sort((a, b) => (degree.get(b) ?? 0) - (degree.get(a) ?? 0) || a.localeCompare(b)).slice(0, cap);
  return ids.map((id) => ({ id, x: 10 + hash(`${seed}:${id}:x`) % 800 / 10, y: 12 + hash(`${seed}:${id}:y`) % 760 / 10 }));
}

export function DreamConstellation({ worker, seed, thumbnail = false, before = false }: {
  worker?: DreamWorker; seed: string; thumbnail?: boolean; before?: boolean;
}) {
  if (!worker || !dreamRunData(worker).hasConstellation) return null;
  const nodes = constellationNodes(worker, seed, 24);
  const points = new Map(nodes.map((node) => [node.id, node]));
  const edges = dreamRunData(worker).edges.filter((edge) => points.has(edge.from) && points.has(edge.to));
  return <svg className={`dream-constellation ${thumbnail ? "dream-thumbnail" : ""}`} viewBox="0 0 100 100" preserveAspectRatio="none"
    role="img" aria-label={before ? "Touched memories before this run" : "Memories and links changed by this run"}>
    {!before && edges.map((edge, index) => <line key={index} x1={points.get(edge.from)!.x} y1={points.get(edge.from)!.y}
      x2={points.get(edge.to)!.x} y2={points.get(edge.to)!.y} stroke="var(--dream-moon)" strokeOpacity=".7" strokeWidth="1.3" vectorEffect="non-scaling-stroke"><title>{edge.relation}</title></line>)}
    {nodes.map(({ id, x, y }) => {
      const action = before ? undefined : worker.node_actions?.[id];
      return <g key={id}><title>{id}</title>
        {action === "merged" && <circle cx={x} cy={y} r="5" fill="var(--dream-moon)" opacity=".14" />}
        <circle cx={x} cy={y} r={action === "merged" ? 2.8 : 1.8} fill={action === "archived" ? "none" : before ? "var(--ct-muted-fg)" : "var(--dream-moon)"}
          opacity={before ? .55 : .85} stroke={action === "archived" ? "var(--ct-muted-fg)" : "none"} vectorEffect="non-scaling-stroke" />
      </g>;
    })}
  </svg>;
}
