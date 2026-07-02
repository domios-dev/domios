import { useEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import type { LaidOutEdge, LaidOutNode, Point } from "../layout";
import type { PresenceAgent } from "../presence/identity";
import type { ViewportTransform } from "../useCanvasViewport";
import type { Task } from "../types";
import { buildReadback, drawScene, hitTestScene, resharpenVisible, type GroupGlyph, type RenderReadbackNode, type SceneExtras, type SectionZoneDraw, type ZoneLabel } from "./drawScene";
import type { HitRegion, RegionAction } from "./drawCard";
import type { SectionTaskGroup } from "./sceneLod";
import { registerBuiltinPainters } from "./builtinPainters";
import { getCanvasPainter } from "./painterRegistry";
import { NodeBitmapCache } from "./nodeBitmapCache";
import { resolveCanvasPalette, type CanvasPalette } from "./palette";
import { bumpCanvasChurn } from "../canvasChurn";

registerBuiltinPainters();

export interface CanvasRenderApi {
  hitTest: (point: Point) => { node: LaidOutNode; action: RegionAction } | null;
  setHover: (nodeId: string | null) => void;
}

export interface CanvasWorldCanvasProps {
  width: number;
  height: number;
  /** Committed camera (gesture-settled) — drives the resharpen pass. */
  transform: ViewportTransform;
  /** Live camera ref — read each rAF blit during a gesture. */
  transformRef: MutableRefObject<ViewportTransform>;
  /** The renderer publishes its blit-scheduler here; the viewport calls it per gesture paint. */
  drawRequestRef: MutableRefObject<(() => void) | null>;
  /** The renderer publishes its imperative hit-test/hover API here for the click/move handlers. */
  apiRef: MutableRefObject<CanvasRenderApi | null>;
  nodes: readonly LaidOutNode[];
  nodeById: Map<string, LaidOutNode>;
  edges: readonly LaidOutEdge[];
  zoneLabels: readonly ZoneLabel[];
  sectionZones?: readonly SectionZoneDraw[];
  groups: readonly GroupGlyph[];
  presenceByNode: ReadonlyMap<string, PresenceAgent[]>;
  resolveModel: (node: LaidOutNode) => unknown;
  /** Bumps when node data changes (forces a content rebuild + resharpen). */
  revision: string;
  /** Transient emphasis (no bitmap invalidation — drawn as primitives). */
  dimIds?: ReadonlySet<string> | null;
  highlightIds?: ReadonlySet<string> | null;
  blinkIds?: ReadonlySet<string> | null;
  dominantId?: string | null;
  lodGroups?: readonly SectionTaskGroup[];
  taskById?: Map<string, Task>;
  /** Fired at the start of every canvas blit with the live camera — keeps world DOM in sync. */
  onCameraPaint?: (t: ViewportTransform) => void;
}

const dprOf = (): number => (typeof window !== "undefined" ? Math.min(window.devicePixelRatio || 1, 2.5) : 1);

export function CanvasWorldCanvas({
  width,
  height,
  transform,
  transformRef,
  drawRequestRef,
  apiRef,
  nodes,
  nodeById,
  edges,
  zoneLabels,
  sectionZones,
  groups,
  presenceByNode,
  resolveModel,
  revision,
  dimIds = null,
  highlightIds = null,
  blinkIds = null,
  dominantId = null,
  lodGroups,
  taskById,
  onCameraPaint,
}: CanvasWorldCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const cacheRef = useRef(new NodeBitmapCache());
  const sigCacheRef = useRef(new Map<string, { model: unknown; sig: string | null }>());
  const rafRef = useRef<number | null>(null);
  const hoverRef = useRef<string | null>(null);
  const dpr = dprOf();

  const [palette, setPalette] = useState<CanvasPalette>(() => resolveCanvasPalette());
  const paletteVersionRef = useRef(palette);
  const paletteRef = useRef(palette);
  paletteRef.current = palette;

  // The painter cache signature per node, memoized by model identity so the per-frame
  // cull/blit never rebuilds it (only re-derives when a node's model object changes).
  const signatureOf = useMemo(() => {
    const cc = sigCacheRef.current;
    return (node: LaidOutNode): string | null => {
      const model = resolveModel(node);
      const prev = cc.get(node.id);
      if (prev && prev.model === model) return prev.sig;
      const sig = getCanvasPainter(node.kind)?.signature(model) ?? null;
      cc.set(node.id, { model, sig });
      return sig;
    };
  }, [resolveModel]);

  // Dispatch the node's paint to its registered canvas painter (the live render seam).
  const paintNode = useMemo(() => {
    return (ctx: CanvasRenderingContext2D, node: LaidOutNode, w: number, h: number): HitRegion[] => {
      const painter = getCanvasPainter(node.kind);
      if (!painter) return [];
      return painter.paint(ctx, { node, model: resolveModel(node), w, h, palette: paletteRef.current });
    };
  }, [resolveModel]);

  const inputRef = useRef({ nodes, edges, nodeById, zoneLabels, sectionZones, groups, presenceByNode, resolveModel, signatureOf, paintNode, cache: cacheRef.current, palette, dpr, width, height, lodGroups, taskById });
  inputRef.current = { nodes, edges, nodeById, zoneLabels, sectionZones, groups, presenceByNode, resolveModel, signatureOf, paintNode, cache: cacheRef.current, palette, dpr, width, height, lodGroups, taskById };
  const extrasRef = useRef<SceneExtras>({ dimIds, highlightIds, blinkIds, dominantId });
  extrasRef.current = { dimIds, highlightIds, blinkIds, dominantId };
  const onCameraPaintRef = useRef(onCameraPaint);
  onCameraPaintRef.current = onCameraPaint;

  const extras = (): SceneExtras => ({ ...extrasRef.current, hoveredId: hoverRef.current });

  const paintCamera = (t: ViewportTransform) => {
    onCameraPaintRef.current?.(t);
  };

  // rAF-coalesced blit at the LIVE camera (one draw per frame regardless of event rate).
  useEffect(() => {
    const blit = () => {
      rafRef.current = null;
      // Live camera (updated every paint) for the perf probe — the canvas has no CSS
      // transform to sniff, and the committed transform is debounced to gesture-end.
      if (typeof window !== "undefined") {
        (window as unknown as { __codetermCamera?: ViewportTransform }).__codetermCamera = transformRef.current;
      }
      const ctx = ctxRef.current;
      if (!ctx) return;
      const t = transformRef.current;
      bumpCanvasChurn("canvasBlits");
      paintCamera(t);
      drawScene(ctx, inputRef.current, t, extras());
    };
    const schedule = () => {
      if (rafRef.current == null) rafRef.current = requestAnimationFrame(blit);
    };
    drawRequestRef.current = schedule;
    apiRef.current = {
      hitTest: (point) => hitTestScene(inputRef.current, transformRef.current, point),
      setHover: (id) => {
        if (hoverRef.current === id) return;
        hoverRef.current = id;
        schedule();
      },
    };
    return () => {
      drawRequestRef.current = null;
      apiRef.current = null;
      if (rafRef.current != null && typeof cancelAnimationFrame !== "undefined") cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    };
  }, [drawRequestRef, apiRef, transformRef]);

  // Size the backing store to the viewport × dpr; CSS size stays in logical px.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.width = Math.max(1, Math.round(width * dpr));
    canvas.height = Math.max(1, Math.round(height * dpr));
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    ctxRef.current = canvas.getContext("2d");
  }, [width, height, dpr]);

  // Theme change → re-resolve the palette and invalidate every bitmap.
  useEffect(() => {
    if (typeof window === "undefined" || typeof MutationObserver === "undefined") return;
    const root = document.documentElement;
    const obs = new MutationObserver(() => setPalette(resolveCanvasPalette()));
    obs.observe(root, { attributes: true, attributeFilter: ["class", "style", "data-theme"] });
    return () => obs.disconnect();
  }, []);

  // Resharpen the cards visible at the COMMITTED camera (gesture-settle / data change),
  // then blit. The blit loop above scale-blits stale bitmaps during the gesture.
  useEffect(() => {
    if (palette !== paletteVersionRef.current) {
      cacheRef.current.clear();
      paletteVersionRef.current = palette;
    }
    bumpCanvasChurn("canvasResharpens");
    resharpenVisible(inputRef.current, transform);
    if (typeof window !== "undefined") {
      const w = window as unknown as { __codetermCanvasRender?: { nodes: RenderReadbackNode[] }; __codetermCamera?: ViewportTransform };
      bumpCanvasChurn("renderReadbackBuilds");
      w.__codetermCanvasRender = { nodes: buildReadback(inputRef.current, transform) };
      w.__codetermCamera = { x: transform.x, y: transform.y, scale: transform.scale };
    }
    const ctx = ctxRef.current;
    if (ctx) {
      paintCamera(transform);
      drawScene(ctx, inputRef.current, transform, extras());
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [transform, nodes, edges, zoneLabels, sectionZones, groups, presenceByNode, palette, dpr, width, height, revision, dimIds, highlightIds, blinkIds, dominantId, lodGroups, taskById]);

  return (
    <canvas
      ref={canvasRef}
      data-testid="canvas-world-canvas"
      style={{ position: "absolute", left: 0, top: 0, zIndex: 2, pointerEvents: "none" }}
    />
  );
}
