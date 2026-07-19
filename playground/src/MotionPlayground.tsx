import { useMemo, useState, type CSSProperties } from "react";
import { CoreConcepts } from "./concepts/core/CoreConcepts";

const ACCENTS = [
  { name: "Electric", value: "#58a6ff" },
  { name: "Mint", value: "#45d483" },
  { name: "Ember", value: "#f58a65" },
  { name: "Violet", value: "#8f84ff" },
];

export function MotionPlayground() {
  const [accent, setAccent] = useState(ACCENTS[0].value);
  const [duration, setDuration] = useState(1.2);
  const [paused, setPaused] = useState(false);
  const [replayKey, setReplayKey] = useState(0);
  const style = useMemo(() => ({
    "--lab-accent": accent,
    "--motion-duration": `${duration}s`,
  }) as CSSProperties, [accent, duration]);

  return (
    <main className={`motion-lab${paused ? " is-paused" : ""}`} style={style}>
      <header className="lab-header">
        <div className="lab-mark" aria-hidden>
          <span />
          <span />
          <span />
        </div>
        <div>
          <p className="lab-kicker">CodeTerm · internal playground</p>
          <h1>Motion Lab</h1>
          <p className="lab-lede">
            Small, legible signals for a team that is actively building. Compare motion in context before it enters the product.
          </p>
        </div>
        <div className="lab-status" role="status">
          <span className="lab-status-pulse" aria-hidden />
          live canvas
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
          <span>Tempo</span>
          <input
            type="range"
            min="0.65"
            max="2.2"
            step="0.05"
            value={duration}
            onChange={(event) => setDuration(Number(event.currentTarget.value))}
          />
          <output>{duration.toFixed(2)}s</output>
        </label>
        <div className="playback-controls">
          <button type="button" onClick={() => setPaused((value) => !value)} aria-pressed={paused}>
            {paused ? "Resume" : "Pause"}
          </button>
          <button type="button" onClick={() => setReplayKey((value) => value + 1)}>Replay all</button>
        </div>
      </section>

      <section className="context-strip" aria-label="Target component preview">
        <div>
          <span className="context-eyebrow">Target surface</span>
          <strong>Team header · someone inside is working</strong>
        </div>
        <div className="context-team">
          <span className="context-avatar" aria-hidden>CT</span>
          <span><b>Motion Design</b><small>2 agents · active</small></span>
          <span className="context-placeholder" aria-hidden>prototype slot</span>
        </div>
      </section>

      <div key={replayKey} className="concept-galleries">
        <CoreConcepts />
      </div>

      <footer className="lab-footer">
        <span>transform + opacity only</span>
        <span>prefers-reduced-motion respected</span>
        <span>keyboard operable controls</span>
      </footer>
    </main>
  );
}
