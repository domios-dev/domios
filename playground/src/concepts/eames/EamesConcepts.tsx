import "./eames-concepts.css";
import type { ReactNode } from "react";

const LANE = "eames";

interface VariantProps {
  id: string;
  number: string;
  name: string;
  note: string;
  aria: string;
  children: ReactNode;
}

function Variant({ id, number, name, note, aria, children }: VariantProps) {
  return (
    <article className="concept-card" data-motion-lane={LANE} data-working-concept={id}>
      <header>
        <span>{number}</span>
        <div><h3>{name}</h3><p>{note}</p></div>
      </header>
      <div className="concept-stage" role="img" aria-label={aria}>
        <div className="mini-team"><b>Team</b><span>active</span></div>
        <div className="concept-motion">
          <div className={`qm-scene qm-${id}`} aria-hidden="true">{children}</div>
        </div>
      </div>
    </article>
  );
}

export function EamesConcepts() {
  return (
    <section className="concept-section eames-concepts" aria-labelledby="eames-concepts-title">
      <div className="section-heading">
        <div>
          <span className="section-index">03</span>
          <h2 id="eames-concepts-title">Quiet Machine · Working</h2>
        </div>
        <p>Built by Eames · five-pose Working grammar</p>
      </div>
      <div className="concept-grid">
        <Variant
          id="tap"
          number="17"
          name="Tap"
          note="A press head strikes the anvil, then rests."
          aria="Working: a press head taps down onto an anvil, then rests, in a slow repeating cycle."
        >
          <span className="qm-anvil" />
          <span className="qm-head" />
        </Variant>
        <Variant
          id="latch"
          number="18"
          name="Latch"
          note="A lever swings down to seat a part, then lifts."
          aria="Working: a lever arm swings down onto a part to seat it, then lifts back to rest, cycling slowly."
        >
          <span className="qm-block" />
          <span className="qm-lever" />
        </Variant>
        <Variant
          id="feed"
          number="19"
          name="Feed"
          note="A carriage advances a piece to the stop, then retracts."
          aria="Working: a carriage slides a piece against a fixed stop and retracts to rest, repeating slowly."
        >
          <span className="qm-stop" />
          <span className="qm-carriage" />
        </Variant>
      </div>
    </section>
  );
}

export default EamesConcepts;
