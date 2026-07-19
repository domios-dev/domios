import "./eames-concepts.css";
import type { ReactNode } from "react";

interface VariantProps {
  id: string;
  number: string;
  name: string;
  note: string;
  aria: string;
  children: ReactNode;
}

const VARIANTS = [
  {
    id: "tap",
    number: "17",
    name: "Tap",
    note: "A press head strikes the anvil, then rests.",
    aria: "Working: a press head taps down onto an anvil, then rests, in a slow repeating cycle.",
    artwork: <><span className="qm-anvil" /><span className="qm-head" /></>,
  },
  {
    id: "latch",
    number: "18",
    name: "Latch",
    note: "A lever swings down to seat a part, then lifts.",
    aria: "Working: a lever arm swings down onto a part to seat it, then lifts back to rest, cycling slowly.",
    artwork: <><span className="qm-block" /><span className="qm-lever" /></>,
  },
  {
    id: "feed",
    number: "19",
    name: "Feed",
    note: "A carriage advances a piece to the stop, then retracts.",
    aria: "Working: a carriage slides a piece against a fixed stop and retracts to rest, repeating slowly.",
    artwork: <><span className="qm-stop" /><span className="qm-carriage" /></>,
  },
] as const;

type EamesVariantId = (typeof VARIANTS)[number]["id"];

interface EamesConceptsProps {
  headingId: string;
  variants?: readonly EamesVariantId[];
}

function Variant({ id, number, name, note, aria, children }: VariantProps) {
  return (
    <article className="concept-card" data-concept={id}>
      <header>
        <span>{number}</span>
        <div><h3>{name}</h3><p>{note}</p></div>
      </header>
      <div className="concept-stage" role="img" aria-label={aria}>
        <div className="mini-team"><b>Team</b><span>active</span></div>
        <div className="concept-motion">
          <div className={`qm-scene qm-${id}`} aria-hidden="true">{children}</div>
        </div>
        <span className="workbench-scale" aria-hidden="true">workbench view</span>
      </div>
    </article>
  );
}

export function EamesConcepts({ headingId, variants = VARIANTS.map(({ id }) => id) }: EamesConceptsProps) {
  return (
    <section className="concept-section eames-concepts" aria-labelledby={headingId}>
      <div className="section-heading">
        <div>
          <span className="section-index">03</span>
          <h2 id={headingId}>Quiet Machine · Working</h2>
        </div>
        <p>Built by Eames · five-pose Working grammar</p>
      </div>
      <div className="concept-grid">
        {VARIANTS.filter(({ id }) => variants.includes(id)).map(({ artwork, ...variant }) => (
          <Variant key={variant.id} {...variant}>{artwork}</Variant>
        ))}
      </div>
    </section>
  );
}

export default EamesConcepts;
