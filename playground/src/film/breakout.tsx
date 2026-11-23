import { createContext, useContext, useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useStage } from "./engine";

/**
 * The farthest a spill ever reaches past the frame, in stage pixels; its canvas is sized for it once. A phone's tall
 * player fills the width of the screen, so there the drawing spills above and below it instead.
 */
const SPILL_REACH = { landscape: { x: 680, y: 420 }, portrait: { x: 60, y: 900 } };
export const spillReach = (portrait: boolean) => (portrait ? SPILL_REACH.portrait : SPILL_REACH.landscape);

/** An unclipped layer over the stage, in stage coordinates, that reaches `margin` past each edge of the frame right now. */
export interface Breakout {
  node: HTMLElement;
  margin: { x: number; y: number };
}

const BreakoutContext = createContext<Breakout | null>(null);
export const BreakoutProvider = BreakoutContext.Provider;
export const useBreakout = () => useContext(BreakoutContext);

/** The spill canvas in stage pixels: the frame sits at (reachX, reachY) inside a canvas `reach` larger on each side. */
export interface SpillGeometry {
  width: number;
  height: number;
  reachX: number;
  reachY: number;
  /** Backing pixels per stage pixel; the spill is seen faded and masked, so it never needs more than this. */
  density: number;
}

const SPILL_DENSITY = 1.25;

/**
 * Draws the same picture again past the frame: the screen keeps its own copy, this one shows only the margin and fades
 * out with distance from the edge, so the drawing reads as larger than the screen. Renders nothing without a layer.
 */
export function SpillSurface({ paint }: { t: number; paint: (canvas: HTMLCanvasElement, geometry: SpillGeometry) => void }) {
  const layer = useBreakout();
  const ref = useRef<HTMLCanvasElement>(null);
  const { width, height, density, portrait } = useStage();
  const mx = layer?.margin.x ?? 0;
  const my = layer?.margin.y ?? 0;
  const { x: rx, y: ry } = spillReach(portrait);
  const w = width + 2 * rx;
  const h = height + 2 * ry;

  useLayoutEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const d = Math.min(density, portrait ? 1 : SPILL_DENSITY);
    const bw = Math.round(w * d);
    const bh = Math.round(h * d);
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width = bw;
      canvas.height = bh;
    }
    paint(canvas, { width, height, reachX: rx, reachY: ry, density: d });
  });

  if (!layer) return null;
  // Opaque at the frame edge, gone `margin` past it; the canvas beyond that stays drawn but unseen.
  const fade = (dir: string, reach: number, margin: number, length: number) =>
    `linear-gradient(${dir}, transparent ${reach - margin}px, #000 ${reach}px, #000 ${length - reach}px, transparent ${length - reach + margin}px)`;
  const mask = `${fade("to right", rx, mx, w)}, ${fade("to bottom", ry, my, h)}`;
  return createPortal(
    <canvas
      ref={ref}
      aria-hidden="true"
      style={{ position: "absolute", left: -rx, top: -ry, width: w, height: h, opacity: 0.8, maskImage: mask, WebkitMaskImage: mask, maskComposite: "intersect", WebkitMaskComposite: "source-in" }}
    />,
    layer.node,
  );
}

/** A 2D drawing in stage coordinates, spilled past the frame. */
export function SpillCanvas({ t, draw }: { t: number; draw: (context: CanvasRenderingContext2D) => void }) {
  return (
    <SpillSurface
      t={t}
      paint={(canvas, { width, height, reachX, reachY, density }) => {
        const context = canvas.getContext("2d");
        if (!context) return;
        context.setTransform(1, 0, 0, 1, 0, 0);
        context.clearRect(0, 0, canvas.width, canvas.height);
        context.setTransform(density, 0, 0, density, reachX * density, reachY * density);
        draw(context);
        context.setTransform(density, 0, 0, density, 0, 0);
        context.clearRect(reachX, reachY, width, height);
      }}
    />
  );
}

/**
 * DOM drawn in a Camera's world, continued past the frame: the breakout copy shows only outside the frame, so the scene's
 * own copy stays the one inside it. Renders nothing without a layer.
 */
export function BreakoutMirror({ camera, world, children }: { camera: { x: number; y: number; scale: number }; world: { width: number; height: number }; children: ReactNode }) {
  const layer = useBreakout();
  const { width, height, portrait } = useStage();
  if (!layer) return null;
  const { x: rx, y: ry } = spillReach(portrait);
  const w = width + 2 * rx;
  const h = height + 2 * ry;
  const { x: mx, y: my } = layer.margin;
  const fade = (dir: string, reach: number, margin: number, length: number) =>
    `linear-gradient(${dir}, transparent ${reach - margin}px, #000 ${reach - margin * 0.6}px, #000 ${length - reach + margin * 0.6}px, transparent ${length - reach + margin}px)`;
  const mask = `${fade("to right", rx, mx, w)}, ${fade("to bottom", ry, my, h)}`;
  const hole = `polygon(evenodd, 0 0, ${w}px 0, ${w}px ${h}px, 0 ${h}px, 0 0, ${rx}px ${ry}px, ${rx}px ${ry + height}px, ${rx + width}px ${ry + height}px, ${rx + width}px ${ry}px, ${rx}px ${ry}px)`;
  return createPortal(
    <div aria-hidden="true" style={{ position: "absolute", left: -rx, top: -ry, width: w, height: h, clipPath: hole, maskImage: mask, WebkitMaskImage: mask, maskComposite: "intersect", WebkitMaskComposite: "source-in", pointerEvents: "none" }}>
      <div style={{ position: "absolute", left: rx + width / 2, top: ry + height / 2, width: world.width, height: world.height, transformOrigin: "0 0", transform: `scale(${camera.scale}) translate(${-camera.x}px, ${-camera.y}px)` }}>
        {children}
      </div>
    </div>,
    layer.node,
  );
}
