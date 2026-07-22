import { useMemo, useState, type CSSProperties } from "react";
import { CoreConcepts } from "./concepts/core/CoreConcepts";
import { EamesConcepts } from "./concepts/eames/EamesConcepts";
import { OpusConcepts } from "./concepts/opus/OpusConcepts";
import { RandWorkingConcepts } from "./concepts/rand/RandWorkingConcepts";

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
  const style = useMemo(() => {
    const scale = duration / 1.2;
    const scaledDuration = (milliseconds: number) => `${Math.round(milliseconds * scale)}ms`;

    return {
      "--lab-accent": accent,
      "--motion-duration": `${duration}s`,
      "--motion-duration-instant": scaledDuration(90),
      "--motion-duration-fast": scaledDuration(150),
      "--motion-duration-moderate": scaledDuration(240),
      "--motion-duration-expressive": scaledDuration(420),
      "--motion-period-ambient": scaledDuration(2800),
      "--motion-duration-glow": scaledDuration(90),
    } as CSSProperties;
  }, [accent, duration]);

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

      <section className="gallery-tier accepted-tier" aria-labelledby="accepted-concepts-title">
        <div className="gallery-tier-heading">
          <div>
            <span className="gallery-tier-index">01</span>
            <h2 id="accepted-concepts-title">Accepted</h2>
          </div>
          <span className="gallery-tier-count">3 concepts</span>
        </div>
        <div key={replayKey} className="concept-galleries">
          <EamesConcepts headingId="accepted-eames-concepts-title" variants={["feed"]} />
          <RandWorkingConcepts headingId="accepted-rand-concepts-title" variants={["quiet-press", "torque-driver"]} />
        </div>
      </section>

      <section className="gallery-tier exploration-tier" aria-labelledby="exploration-concepts-title">
        <div className="gallery-tier-heading">
          <div>
            <span className="gallery-tier-index">02</span>
            <h2 id="exploration-concepts-title">Exploration</h2>
          </div>
          <div>
            <span className="gallery-tier-count">19 concepts</span>
            <p>Rejected references</p>
          </div>
        </div>
        <div key={replayKey} className="concept-galleries">
          <CoreConcepts />
          <OpusConcepts />
          <EamesConcepts headingId="exploration-eames-concepts-title" variants={["tap", "latch"]} />
          <RandWorkingConcepts headingId="exploration-rand-concepts-title" variants={["bench-clamp"]} />
        </div>
      </section>

      <footer className="lab-footer">
        <span>watch the whole loop</span>
        <span>develop large · verify small</span>
        <span>feeling before conformance</span>
      </footer>
    </main>
  );
}
