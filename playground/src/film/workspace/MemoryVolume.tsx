import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { holoFrame, HOOK_CAST } from "../scenes/lab/lab-memory-hook.scene";
import type { Holo } from "../scenes/mesh/holoGl";

type Renderer = typeof import("../scenes/mesh/holoGl");

export default memo(function MemoryVolume({ paused, reducedMotion, onFailed }: {
  paused: boolean; reducedMotion: boolean; onFailed: () => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const renderer = useRef<{ value: Holo; portrait: boolean; light: boolean; narrow: boolean } | null>(null);
  const lastPointer = useRef<{ id: number; x: number; y: number } | null>(null);
  const [module, setModule] = useState<Renderer | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0, light: false });
  const orbit = useRef({ yaw: 0, pitch: 0, elapsed: 0, manual: false });
  const redraw = useRef<(() => void) | null>(null);

  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    let mounted = true;
    void import("../scenes/mesh/holoGl").then((loaded) => {
      if (mounted) setModule(loaded);
    }, () => { if (mounted) onFailed(); });
    const root = element.closest<HTMLElement>(".domios-film-root");
    const measure = () => {
      const { clientWidth: width, clientHeight: height } = element;
      const light = root?.dataset.ctTheme === "light";
      setSize((previous) => previous.width === width && previous.height === height && previous.light === light
        ? previous : { width, height, light });
    };
    const layout = new ResizeObserver(measure);
    const theme = new MutationObserver(measure);
    layout.observe(element);
    if (root) theme.observe(root, { attributes: true });
    measure();
    return () => {
      mounted = false;
      theme.disconnect();
      layout.disconnect();
      renderer.current?.value.dispose();
      renderer.current = null;
    };
  }, [onFailed]);

  useLayoutEffect(() => {
    const element = canvas.current;
    const { width, height, light } = size;
    if (!element || !module || !width || !height) return;
    const portrait = height > width;
    const narrow = width < 650;
    const frame = holoFrame(portrait);
    let engine = renderer.current;
    if (!engine || engine.portrait !== portrait || engine.light !== light || engine.narrow !== narrow) {
      engine?.value.dispose(false);
      const budget = narrow ? { points: 620, edges: 1500 } : { points: 950, edges: 2500 };
      const value = module.Holo.create(element, frame.nodes, light, HOOK_CAST, false, budget);
      if (!value) { renderer.current = null; onFailed(); return; }
      renderer.current = engine = { value, portrait, light, narrow };
    }

    const { value } = engine;
    const view = { ...frame.wide, scale: frame.wide.scale * Math.min(width / frame.width, height / frame.height) * 1.24 };
    const dpr = Math.min(window.devicePixelRatio || 1, narrow ? 1 : 1.25, Math.sqrt(900_000 / (width * height)));
    let animation: number | null = null;
    let previous: number | null = null;

    const paint = (now: number) => {
      animation = null;
      const pose = orbit.current;
      const moving = !paused && !reducedMotion && !pose.manual;
      if (moving && previous !== null) pose.elapsed += Math.min(50, now - previous);
      previous = now;
      const drift = reducedMotion ? 0 : Math.sin(pose.elapsed * 0.00025) * 0.35;
      value.render({
        view, burst: 12, flatten: 0, holo: 1, yaw: 0.38 + drift + pose.yaw, pitch: 0.26 + pose.pitch,
        ego: 0.65, scan: null, ring: null, opacity: 1,
      }, width, height, dpr);
      if (moving) requestPaint();
    };
    const requestPaint = () => {
      if (animation === null) animation = requestAnimationFrame(paint);
    };

    redraw.current = requestPaint;
    requestPaint();
    return () => {
      redraw.current = null;
      if (animation !== null) cancelAnimationFrame(animation);
    };
  }, [module, size, paused, reducedMotion, onFailed]);

  const rotate = (yaw: number, pitch: number) => {
    const pose = orbit.current;
    pose.manual = true;
    pose.yaw += yaw;
    pose.pitch = Math.max(-0.86, Math.min(0.86, pose.pitch + pitch));
    redraw.current?.();
  };

  return <div className="absolute inset-0">
    <canvas ref={canvas} tabIndex={0} aria-label="3D memory graph. Drag or use arrow keys to rotate."
      className="absolute inset-0 block h-full w-full touch-pan-y cursor-grab active:cursor-grabbing"
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        lastPointer.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
      }}
      onPointerMove={(event) => {
        const last = lastPointer.current;
        if (last?.id !== event.pointerId) return;
        rotate((event.clientX - last.x) * 0.008, (event.clientY - last.y) * 0.008);
        lastPointer.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
      }}
      onLostPointerCapture={() => { lastPointer.current = null; }}
      onKeyDown={(event) => {
        const angles: Record<string, [number, number]> = { ArrowLeft: [-0.15, 0], ArrowRight: [0.15, 0], ArrowUp: [0, -0.12], ArrowDown: [0, 0.12] };
        const angle = angles[event.key];
        if (!angle) return;
        event.preventDefault();
        rotate(...angle);
      }} />
    <small className="pointer-events-none absolute bottom-4 left-4 text-[var(--ct-muted-fg)]">Drag to rotate</small>
  </div>;
});
