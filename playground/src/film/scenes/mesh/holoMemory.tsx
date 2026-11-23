import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { SpillSurface } from "../../breakout";
import { useStage } from "../../engine";
import { themeIsLight, type HoloCast, type HoloState, type Layout } from "./holoTheme";

type Gl = typeof import("./holoGl");
type Instance = import("./holoGl").Holo;

// three.js and the hologram load as their own chunk, started when the first hologram canvas mounts (the poster), not when
// the page loads the film module, so the page's own start never pays for them.
let loaded: Gl | null = null;
let loading: Promise<Gl | null> | null = null;
const startLoading = () =>
  (loading ??= import("./holoGl").then(
    (gl) => (loaded = gl),
    () => null,
  ));

/** The stage copy never needs more backing pixels than this per stage pixel. */
const STAGE_DENSITY = 2;

type Held = { instance: Instance; canvas: HTMLCanvasElement; layout: Layout; light: boolean; cast: string } | null;

/** One hologram renderer per canvas, rebuilt when the canvas, layout, theme or cast changes; null without WebGL. */
function useHolo(spill = false) {
  const held = useRef<Held>(null);
  useLayoutEffect(() => () => {
    held.current?.instance.dispose();
    held.current = null;
  }, []);
  return (gl: Gl, canvas: HTMLCanvasElement, layout: Layout, cast: HoloCast): Instance | null => {
    const light = themeIsLight();
    const key = `${cast.ego}|${cast.fresh}`;
    const h = held.current;
    if (h && h.canvas === canvas && h.layout === layout && h.light === light && h.cast === key) return h.instance;
    // A new canvas frees the old context; the same canvas keeps it for the renderer that replaces this one.
    h?.instance.dispose(h.canvas !== canvas);
    held.current = null;
    const instance = gl.Holo.create(canvas, layout, light, cast, spill);
    if (instance) held.current = { instance, canvas, layout, light, cast: key };
    return instance;
  };
}

/** The same hologram again past the frame, at the same scale, on the film's breakout layer; nothing without one. */
function HoloSpill({ t, state, layout, cast, gl }: { t: number; state: HoloState; layout: Layout; cast: HoloCast; gl: Gl }) {
  const holo = useHolo(true);
  return (
    <SpillSurface
      t={t}
      paint={(canvas, { width, height, reachX, reachY, density }) => {
        holo(gl, canvas, layout, cast)?.render(state, width, height, density, { reachX, reachY });
      }}
    />
  );
}

/**
 * The hologram on one stage-sized canvas, redrawn for every t, plus its spill past the frame. `onReady(false)` means the
 * flat graph should cover the cut instead: the chunk has not arrived yet, or the browser can't give it a WebGL context.
 */
export function HoloCanvas({ t, state, layout, cast, onReady, onFailed }: { t: number; state: HoloState; layout: Layout; cast: HoloCast; onReady: (ok: boolean) => void; onFailed?: () => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const { width, height, density: screen } = useStage();
  const density = Math.min(screen, STAGE_DENSITY);
  const [gl, setGl] = useState<Gl | null>(loaded);
  const [failed, setFailed] = useState(false);
  const holo = useHolo();

  useEffect(() => {
    if (gl) return;
    let live = true;
    void startLoading().then((module) => {
      if (!live) return;
      if (module) setGl(module);
      else setFailed(true);
    });
    return () => {
      live = false;
    };
  }, [gl]);

  useEffect(() => onReady(!!gl && !failed), [gl, failed]);
  useEffect(() => {
    if (failed) onFailed?.();
  }, [failed]);

  useLayoutEffect(() => {
    const canvas = ref.current;
    if (!canvas || !gl || failed) return;
    const instance = holo(gl, canvas, layout, cast);
    if (!instance) {
      setFailed(true);
      return;
    }
    instance.render(state, width, height, density);
  }, [t, width, height, density, layout, gl, failed]);

  if (failed) return null;
  return (
    <>
      <canvas ref={ref} aria-hidden="true" data-film-hint="Memory graph: a year of what your agents learned, linked by meaning" style={{ position: "absolute", inset: 0, width, height, opacity: gl && state.opacity > 0 ? 1 : 0 }} />
      {gl && state.opacity > 0 && <HoloSpill t={t} state={state} layout={layout} cast={cast} gl={gl} />}
    </>
  );
}
