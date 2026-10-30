import { useEffect, useRef, type RefObject } from "react";
import type Sigma from "sigma";
import type { MemNode } from "./types";

export function useProvenanceRings(sigmaRef: RefObject<Sigma | null>, nodes?: Map<string, MemNode>) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const sigma = sigmaRef.current;
    const canvas = canvasRef.current;
    if (!sigma || !canvas) return;
    const ids = [...(nodes?.values() ?? [])].filter(node => node.author?.kind === "neighbor").map(node => node.id);
    const draw = () => {
      const { width, height } = sigma.getDimensions();
      const ratio = window.devicePixelRatio || 1;
      const pixelWidth = Math.round(width * ratio);
      const pixelHeight = Math.round(height * ratio);
      if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
        canvas.width = pixelWidth;
        canvas.height = pixelHeight;
      }
      const context = canvas.getContext("2d");
      if (!context) return;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      context.strokeStyle = getComputedStyle(canvas).getPropertyValue("--ct-accent").trim();
      context.lineWidth = 1.5;
      for (const id of ids) {
        const data = sigma.getNodeDisplayData(id);
        if (!data || data.hidden) continue;
        const point = sigma.framedGraphToViewport(data);
        if (point.x < 0 || point.y < 0 || point.x > width || point.y > height) continue;
        context.beginPath();
        context.arc(point.x, point.y, sigma.scaleSize(data.size) + 3, 0, Math.PI * 2);
        context.stroke();
      }
    };
    sigma.on("afterRender", draw);
    draw();
    return () => { sigma.off("afterRender", draw); };
  }, [sigmaRef, nodes]);
  return canvasRef;
}
