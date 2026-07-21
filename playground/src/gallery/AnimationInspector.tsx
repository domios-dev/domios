import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useAnimationFreeze } from "./useAnimationFreeze";
import type { Specimen } from "./types";

// Scales any specimen down to fit its box — wide components (status rows)
// shrink; small ones (dots) render 1:1. Scale is transform-only, so frozen
// animation phases stay exact.
export function FitBox({ children, pad = 16 }: { children: ReactNode; pad?: number }) {
  const outer = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  useLayoutEffect(() => {
    const o = outer.current;
    const i = inner.current;
    if (!o || !i) return;
    const measure = () => {
      const w = o.clientWidth;
      if (!w) return; // grid not laid out yet — wait for the observer
      // Never below 0.45: a legible crop beats a complete-but-microscopic fit.
      setScale(Math.max(0.45, Math.min(1, (w - pad) / Math.max(1, i.scrollWidth))));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(o);
    ro.observe(i);
    return () => ro.disconnect();
  }, [pad]);
  return (
    <div ref={outer} className="gallery-fitbox">
      <div ref={inner} className="gallery-fitbox-inner" style={{ transform: `scale(${scale})` }}>
        {children}
      </div>
    </div>
  );
}

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
        <FitBox pad={20}>
          <Component {...state.props} />
        </FitBox>
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
  const otherStates = specimen.states.filter((entry) => entry.id !== state.id).map((entry) => entry.label);
  return (
    <section className="gallery-inspector" aria-labelledby="animation-inspector-title">
      <div className="gallery-section-heading">
        <div>
          <p className="portal-kicker">motion tool</p>
          <h2 id="animation-inspector-title">Animation inspector</h2>
        </div>
        <span className="gallery-animation-count">{controls.animationCount ? `${controls.animationCount} animations · ${Math.round(controls.cycleMs)} ms` : "Focused state is still"}</span>
      </div>
      {controls.animationCount ? <>
        <ScrubBar controls={controls} />
        <Filmstrip specimen={specimen} state={state} cycleMs={controls.cycleMs} />
      </> : <p className="gallery-inspector-note">{otherStates.length ? `Choose another state to inspect its motion: ${otherStates.join(", ")}.` : "This specimen has no other states to inspect."}</p>}
    </section>
  );
}

export function StaticComponentNote() {
  return <p className="gallery-static-note">Static component — its states are shown above without an animation inspector.</p>;
}
