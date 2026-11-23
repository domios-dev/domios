import { BACKDROPS, DEFAULT_BACKDROP } from "../backdrops";
import { StageProvider, TimeProvider, type Scene } from "../engine";

export interface FrameProps {
  scene: Scene;
  t: number;
  width: number;
  height: number;
  pulses: number[];
  backdrop?: string;
  intensity?: number;
  /** How large the stage is drawn on screen relative to its own pixels. */
  scale?: number;
}

/** One stage-sized frame of a scene over its backdrop; the playground and the embed render the same tree. */
export function Frame({ scene, t, width, height, pulses, backdrop = DEFAULT_BACKDROP, intensity = 0.75, scale }: FrameProps) {
  const Backdrop = BACKDROPS[backdrop] ?? BACKDROPS.none;
  return (
    <div style={{ position: "relative", width, height, overflow: "hidden", background: "var(--ct-bg)" }}>
      <StageProvider width={width} height={height} density={scale === undefined ? undefined : scale * (window.devicePixelRatio > 0 ? window.devicePixelRatio : 1)}>
        <Backdrop t={t} width={width} height={height} pulses={pulses} intensity={intensity} />
        <TimeProvider t={t}>{scene.render(t)}</TimeProvider>
      </StageProvider>
    </div>
  );
}
