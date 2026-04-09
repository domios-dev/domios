export interface Viewport {
  panX: number;
  panY: number;
  scale: number;
}

export interface CanvasSize {
  width: number;
  height: number;
}

interface NodeLike {
  id: string;
  graph_x?: number | null;
  graph_y?: number | null;
}

export function getVisibleNodeIds(
  nodes: NodeLike[],
  viewport: Viewport,
  canvas: CanvasSize,
  opts: { padding?: number } = {},
): string[] {
  const padding = opts.padding ?? 0;
  const worldRect = {
    x: (-viewport.panX - padding) / viewport.scale,
    y: (-viewport.panY - padding) / viewport.scale,
    width: (canvas.width + padding * 2) / viewport.scale,
    height: (canvas.height + padding * 2) / viewport.scale,
  };

  const vx1 = worldRect.x;
  const vy1 = worldRect.y;
  const vx2 = worldRect.x + worldRect.width;
  const vy2 = worldRect.y + worldRect.height;

  const visible: string[] = [];
  for (const n of nodes) {
    if (n.graph_x == null || n.graph_y == null) continue;
    const nx1 = n.graph_x - 120;
    const ny1 = n.graph_y - 60;
    const nx2 = n.graph_x + 120;
    const ny2 = n.graph_y + 60;
    if (nx2 < vx1 || nx1 > vx2 || ny2 < vy1 || ny1 > vy2) continue;
    visible.push(n.id);
  }
  return visible;
}
