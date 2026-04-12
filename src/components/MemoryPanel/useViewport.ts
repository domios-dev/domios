import { useCallback, useState } from "react";
import type { MemNode, Viewport } from "./types";

export function useViewport(initial: Viewport = { panX: 0, panY: 0, scale: 1 }) {
  const [viewport, setViewport] = useState<Viewport>(initial);

  const onWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    const delta = -e.deltaY * 0.001;
    const rect = e.currentTarget.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    setViewport((v) => {
      const newScale = Math.max(0.1, Math.min(5, v.scale * (1 + delta)));
      const worldX = (mx - v.panX) / v.scale;
      const worldY = (my - v.panY) / v.scale;
      return {
        panX: mx - worldX * newScale,
        panY: my - worldY * newScale,
        scale: newScale,
      };
    });
  }, []);

  const onPanStart = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    const start = viewport;
    const move = (ev: MouseEvent) => {
      setViewport({
        ...start,
        panX: start.panX + (ev.clientX - startX),
        panY: start.panY + (ev.clientY - startY),
      });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }, [viewport]);

  const fitToView = useCallback(
    (nodes: Iterable<MemNode>, viewWidth: number, viewHeight: number) => {
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      let count = 0;
      for (const n of nodes) {
        if (n.graph_x == null || n.graph_y == null) continue;
        if (n.graph_x < minX) minX = n.graph_x;
        if (n.graph_y < minY) minY = n.graph_y;
        if (n.graph_x > maxX) maxX = n.graph_x;
        if (n.graph_y > maxY) maxY = n.graph_y;
        count++;
      }
      if (count === 0 || viewWidth <= 0 || viewHeight <= 0) return;
      const pad = 120;
      const w = Math.max(1, maxX - minX) + pad * 2;
      const h = Math.max(1, maxY - minY) + pad * 2;
      const scale = Math.max(0.02, Math.min(5, Math.min(viewWidth / w, viewHeight / h)));
      const cx = (minX + maxX) / 2;
      const cy = (minY + maxY) / 2;
      setViewport({
        panX: viewWidth / 2 - cx * scale,
        panY: viewHeight / 2 - cy * scale,
        scale,
      });
    },
    [],
  );

  const recenter = useCallback(() => {
    setViewport({ panX: 0, panY: 0, scale: 1 });
  }, []);

  return { viewport, setViewport, onWheel, onPanStart, fitToView, recenter };
}
