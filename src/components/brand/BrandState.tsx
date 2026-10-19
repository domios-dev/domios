import { useId, type CSSProperties, type ReactNode } from "react";
import { BrandMark } from "./BrandMark";
import type { BrandStateModel } from "./brandStateModel";
import { usePrefersReducedMotion } from "./usePrefersReducedMotion";

export function BrandState({ model, className, children }: {
  model: BrandStateModel;
  className?: string;
  children?: ReactNode;
}) {
  const reducedMotion = usePrefersReducedMotion();
  const headingId = useId();

  if (model.kind === "hidden") return null;
  if (model.kind === "loading") {
    return (
      <div className={`brand-state brand-state--loading ${className ?? ""}`} role="status" aria-live="polite" aria-label={model.label} aria-busy="true">
        <BrandMark motion={model.animate && !reducedMotion ? "load" : "static"} size={92} />
        {model.animate && !reducedMotion && <div className="brand-state-dots" aria-hidden="true">{[0, 1, 2, 3, 4, 5, 6].map(index => <span key={index} style={{ "--dot-index": index } as CSSProperties} />)}</div>}
        <div className="brand-state-copy">
          <h2>{model.label}</h2>
          {model.detail && <p>{model.detail}</p>}
        </div>
        {children}
      </div>
    );
  }
  if (model.kind === "error") {
    return (
      <section className={`brand-state brand-state--error ${className ?? ""}`} role="alert" aria-live="assertive" aria-labelledby={headingId}>
        <BrandMark motion="static" size={76} />
        <h2 id={headingId}>{model.title}</h2>
        <p>{model.detail}</p>
        {children}
      </section>
    );
  }
  if (model.kind === "reconnecting") {
    return (
      <div className={`brand-connection-scrim ${className ?? ""}`}>
        <div className="brand-connection-card" role="status" aria-live="polite">
          <BrandMark motion={reducedMotion ? "static" : "load"} size={34} />
          <span>Reconnecting…</span>
        </div>
      </div>
    );
  }
  return (
    <div className={`brand-reconnected ${className ?? ""}`} role="status" aria-live="polite">
      <BrandMark motion="static" size={18} />
      <span>Reconnected</span>
    </div>
  );
}
