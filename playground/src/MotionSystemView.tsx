import { useMemo, useState, type CSSProperties } from "react";
import { MotionSystemConcept } from "./concepts/system/MotionSystemConcept";

const ACCENTS = [
  { name: "Electric", value: "#58a6ff" },
  { name: "Mint", value: "#45d483" },
  { name: "Ember", value: "#f58a65" },
  { name: "Violet", value: "#8f84ff" },
];

export function MotionSystemView() {
  const [accent, setAccent] = useState(ACCENTS[0].value);
  const [duration, setDuration] = useState(1);
  const [paused, setPaused] = useState(false);
  const [replayKey, setReplayKey] = useState(0);

  const style = useMemo(() => {
    const scaled = (ms: number) => `${Math.round(ms * duration)}ms`;
    return {
      "--lab-accent": accent,
      "--motion-duration-instant": scaled(70),
      "--motion-duration-fast": scaled(120),
      "--motion-duration-moderate": scaled(220),
      "--motion-duration-expressive": scaled(320),
      "--motion-period-ambient": scaled(1400),
      "--motion-ease-standard": "cubic-bezier(0.2, 0, 0.38, 0.9)",
      "--motion-ease-enter": "cubic-bezier(0, 0, 0.38, 0.9)",
      "--motion-ease-exit": "cubic-bezier(0.2, 0, 1, 0.9)",
    } as CSSProperties;
  }, [accent, duration]);

  return (
    <main className={`motion-lab${paused ? " is-paused" : ""}`} style={style}>
      <header className="lab-header">
        <div className="lab-mark" aria-hidden>
          <span /><span /><span />
        </div>
        <div>
          <p className="lab-kicker">CodeTerm · unified motion concept</p>
          <h1>Motion System</h1>
          <p className="lab-lede">
            One interaction language across the surfaces CodeTerm actually uses. Motion here explains
            state and causality — selection, disclosure, liveness, overlays, sending — never decorates.
            Everything below is driven by the shared tokens; the Tempo control slows the whole system at once.
          </p>
        </div>
        <div className="lab-status" role="status">
          <span className="lab-status-pulse" aria-hidden />
          live concept
        </div>
      </header>

      <section className="lab-controls" aria-label="Motion controls">
        <fieldset className="accent-control">
          <legend>Theme accent</legend>
          <div className="accent-swatches">
            {ACCENTS.map((item) => (
              <button
                key={item.value}
                type="button"
                className="accent-swatch"
                style={{ "--swatch": item.value } as CSSProperties}
                aria-label={`${item.name} accent`}
                aria-pressed={accent === item.value}
                onClick={() => setAccent(item.value)}
              />
            ))}
          </div>
        </fieldset>
        <label className="speed-control">
          <span>Tempo (1× real → 4× slow-mo)</span>
          <input type="range" min="1" max="4" step="0.25" value={duration} onChange={(e) => setDuration(Number(e.currentTarget.value))} />
          <output>{duration.toFixed(2)}×</output>
        </label>
        <div className="playback-controls">
          <button type="button" onClick={() => setPaused((v) => !v)} aria-pressed={paused}>{paused ? "Resume" : "Pause"}</button>
          <button type="button" onClick={() => setReplayKey((v) => v + 1)}>Replay</button>
        </div>
      </section>

      <div key={replayKey} style={{ marginTop: 20 }}>
        <MotionSystemConcept />
      </div>

      <footer className="lab-footer">
        <span>one room, not one strobe</span>
        <span>origin-true overlays</span>
        <span>content is sovereign</span>
        <span>quiet by default</span>
      </footer>
    </main>
  );
}
