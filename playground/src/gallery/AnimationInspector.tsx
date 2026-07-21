import { useLayoutEffect, useRef, useState } from "react";
import { useAnimationFreeze } from "./useAnimationFreeze";
import type { Specimen } from "./types";

type FreezeControls = ReturnType<typeof useAnimationFreeze>;

export function ScrubBar({ controls }: { controls: FreezeControls }) {
  const [time, setTime] = useState(0);
  const [paused, setPaused] = useState(false);

  const scrub = (next: number) => {
    setTime(next);
    setPaused(true);
    controls.pauseAt(next);
  };

  const togglePlayback = () => {
    if (paused) {
      controls.resume();
      setPaused(false);
      return;
    }
    controls.pauseAt(time);
    setPaused(true);
  };

  return (
    <div className="gallery-scrub">
      <button className="gallery-play" type="button" onClick={togglePlayback} aria-label={paused ? "Play animation" : "Pause animation"}>
        {paused ? "Play" : "Pause"}
      </button>
      <input
        aria-label="Animation timeline"
        type="range"
        min="0"
        max={controls.cycleMs}
        value={Math.min(time, controls.cycleMs)}
        onChange={(event) => scrub(Number(event.currentTarget.value))}
      />
      <output>{Math.round(Math.min(time, controls.cycleMs))} ms</output>
    </div>
  );
}

function FrozenFrame({ specimen, state, time }: { specimen: Specimen; state: Specimen["states"][number]; time: number }) {
  const frame = useRef<HTMLDivElement>(null);
  const controls = useAnimationFreeze(frame, [specimen.id, state.id]);

  useLayoutEffect(() => {
    controls.pauseAt(time);
  }, [controls, time]);

  const Component = specimen.Component;
  return (
    <figure className="gallery-frame">
      <div className="gallery-frame-stage" ref={frame}>
        <Component {...state.props} />
      </div>
      <figcaption>{Math.round(time)} ms</figcaption>
    </figure>
  );
}

export function Filmstrip({ specimen, state, cycleMs, n = 6 }: { specimen: Specimen; state: Specimen["states"][number]; cycleMs: number; n?: number }) {
  const frames = Array.from({ length: n }, (_, index) => (cycleMs * index) / Math.max(1, n - 1));
  return (
    <div className="gallery-filmstrip" aria-label={`${n} animation frames`}>
      {frames.map((time, index) => <FrozenFrame key={`${state.id}-${index}`} specimen={specimen} state={state} time={time} />)}
    </div>
  );
}

export function AnimationInspector({ specimen, state, stage }: { specimen: Specimen; state: Specimen["states"][number]; stage: React.RefObject<HTMLDivElement | null> }) {
  const controls = useAnimationFreeze(stage, [specimen.id, state.id]);
  return (
    <section className="gallery-inspector" aria-labelledby="animation-inspector-title">
      <div className="gallery-section-heading">
        <div>
          <p className="portal-kicker">motion tool</p>
          <h2 id="animation-inspector-title">Animation inspector</h2>
        </div>
        <span className="gallery-animation-count">{controls.animationCount ? `${controls.animationCount} animations · ${Math.round(controls.cycleMs)} ms` : "No browser animations found"}</span>
      </div>
      <ScrubBar controls={controls} />
      <Filmstrip specimen={specimen} state={state} cycleMs={controls.cycleMs} />
    </section>
  );
}
