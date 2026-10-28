import { useId, useRef, type ReactNode } from "react";
import { BrandMark } from "./BrandMark";
import { DotMatrixCanvas } from "./DotMatrixCanvas";
import type { DotMatrixCanvasHandle } from "./DotMatrixCanvas";
import type { BrandStateModel } from "./brandStateModel";
import { usePrefersReducedMotion } from "./usePrefersReducedMotion";

export function BrandState({ model, className, children, pulse = false, markSize = 92 }: {
  model: BrandStateModel;
  className?: string;
  children?: ReactNode;
  /** Beat the mark and release dot waves; off by default so the boot cover hands over without a restart. */
  pulse?: boolean;
  /** The loading mark's size; Loader's compact variant draws it smaller. */
  markSize?: number;
}) {
  const reducedMotion = usePrefersReducedMotion();
  const headingId = useId();
  const matrixRef = useRef<DotMatrixCanvasHandle>(null);
  const emitRelease = (timestamp: number) => matrixRef.current?.emitRelease(timestamp);

  if (model.kind === "hidden") return null;
  if (model.kind === "loading") {
    return (
      <div className={`brand-state brand-state--loading ${className ?? ""}`} role="status" aria-live="polite" aria-label={model.label} aria-busy="true">
        <DotMatrixCanvas ref={matrixRef} mode="loading" reducedMotion={reducedMotion || !model.animate} />
        <BrandMark motion={pulse && model.animate && !reducedMotion ? "load" : "static"} onRelease={emitRelease} size={markSize} />
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
        <DotMatrixCanvas mode="error" reducedMotion={reducedMotion} />
        <BrandMark motion="static" size={76} />
        <div className="brand-state-error-copy">
          <h2 id={headingId} tabIndex={-1}>{model.title}</h2>
          <p>{model.sentence}</p>
        </div>
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
