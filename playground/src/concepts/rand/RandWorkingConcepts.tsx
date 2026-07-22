import "./rand-working-concepts.css";

const VARIANTS = [
  {
    id: "quiet-press",
    number: "17",
    name: "Quiet press",
    note: "A guided ram makes one measured press, then rests.",
    label: "Working: a compact press lowers its ram onto the machine bed, then returns to rest.",
  },
  {
    id: "bench-clamp",
    number: "18",
    name: "Bench clamp",
    note: "A sliding jaw closes once against a fixed stop.",
    label: "Working: a bench clamp slides its jaw toward the fixed stop, settles, then rests.",
  },
  {
    id: "torque-driver",
    number: "19",
    name: "Torque driver",
    note: "A short driver seats the fixed fastener once.",
    label: "Working: a short driver seats a fixed fastener with one restrained stroke, then rests.",
  },
] as const;

type RandVariantId = (typeof VARIANTS)[number]["id"];

interface RandWorkingConceptsProps {
  headingId: string;
  variants?: readonly RandVariantId[];
}

export function RandWorkingConcepts({ headingId, variants = VARIANTS.map(({ id }) => id) }: RandWorkingConceptsProps) {
  return (
    <section className="concept-section rand-concepts" aria-labelledby={headingId}>
      <div className="section-heading">
        <div>
          <span className="section-index">03</span>
          <h2 id={headingId}>Quiet machine</h2>
        </div>
        <p>Built by Rand · grounded working gestures</p>
      </div>
      <div className="concept-grid">
        {VARIANTS.filter(({ id }) => variants.includes(id)).map((variant) => (
          <article
            className="concept-card"
            data-concept={variant.id}
            key={variant.id}
          >
            <header>
              <span>{variant.number}</span>
              <div>
                <h3>{variant.name}</h3>
                <p>{variant.note}</p>
              </div>
            </header>
            <div className="concept-stage" role="img" aria-label={variant.label}>
              <div className="mini-team">
                <b>Team</b>
                <span>active</span>
              </div>
              <div className="concept-motion rand-motion">
                <div
                  className={`rand-stage rand-${variant.id}`}
                  aria-hidden="true"
                >
                  <span className="rand-anchor" aria-hidden="true" />
                  <span className="rand-active" aria-hidden="true" />
                </div>
              </div>
              <span className="workbench-scale" aria-hidden="true">workbench view</span>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
