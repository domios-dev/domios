import { useEffect, useMemo, useState } from "react";
import { ProductProviders } from "../../gallery/fixtures";
import type { Scene } from "../engine";
import { PortalStage } from "../fixtures/stage";
import type { Cut } from "../reels/library";
import { Frame } from "./frame";
import { useHints } from "./hints";
import { STAGE_CONTAINER, STAGE_VH, STAGE_VW } from "./rootClass";
import { __film, setStage } from "./scope";
import { useScale, useWheelPassThrough } from "./stageHooks";
import { loopFade, loopWindow } from "./timeline";

export interface SceneCardController {
  setPaused: (paused: boolean) => void;
}

export interface SceneCardProps {
  reel: Scene;
  cut: string;
  window?: [number, number];
  reducedMotion: boolean;
  controller: SceneCardController;
}

const WIDTH = 1920;
const HEIGHT = 1080;

/** One cut of the reel as a silent live loop: its own clock, paused off screen, one held frame under reduced motion. */
export function SceneCard({ reel, cut, window: range, reducedMotion, controller }: SceneCardProps) {
  setStage(WIDTH, HEIGHT);
  const cuts = (__film.globals.__filmCuts as Record<string, Cut[]> | undefined)?.[reel.id] ?? [];
  const span = useMemo(() => loopWindow(cuts.find((c) => c.id === cut), range), [cuts, cut, range]);
  const [box, setBox] = useState<HTMLDivElement | null>(null);
  const [stageEl, setStageEl] = useState<HTMLDivElement | null>(null);
  const scale = useScale(box, WIDTH);
  const [local, setLocal] = useState(() => (span ? (span.to - span.from) / 2 : 0));
  const [hostPaused, setHostPaused] = useState(false);
  const [visible, setVisible] = useState(false);
  useWheelPassThrough(box);
  const hints = useHints(stageEl, true);

  useEffect(() => {
    controller.setPaused = setHostPaused;
  }, [controller]);

  useEffect(() => {
    if (!box) return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting));
    observer.observe(box);
    return () => observer.disconnect();
  }, [box]);

  const running = !!span && visible && !hostPaused && !reducedMotion;
  useEffect(() => {
    if (!running || !span) return;
    const length = span.to - span.from;
    const origin = performance.now() - local * 1000;
    let raf = 0;
    const tick = (now: number) => {
      setLocal(((now - origin) / 1000) % length);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [running, span]);

  if (!span) return null;
  const opacity = reducedMotion ? 1 : loopFade(local, span.to - span.from);

  return (
    <ProductProviders>
      <div ref={setBox} className="film-box film-card-box" data-reduced-motion={reducedMotion || undefined}>
        <div ref={setStageEl} className="film-stage film-card-stage">
          {scale > 0 && (
            <div
              className="film-stage-inner"
              style={{ width: WIDTH, height: HEIGHT, opacity, transform: `scale(${scale})`, containerName: STAGE_CONTAINER, [STAGE_VW as string]: `${WIDTH / 100}px`, [STAGE_VH as string]: `${HEIGHT / 100}px` }}
            >
              <div inert aria-hidden="true" style={{ position: "absolute", inset: 0 }}>
                <PortalStage>
                  <Frame scene={reel} t={span.from + local} width={WIDTH} height={HEIGHT} pulses={[]} scale={scale} />
                </PortalStage>
              </div>
            </div>
          )}
        </div>
        <div ref={hints.ring} className="film-hint-ring" aria-hidden="true" />
        <div ref={hints.chip} className="film-hint" aria-hidden="true"><b /><span /></div>
      </div>
    </ProductProviders>
  );
}
