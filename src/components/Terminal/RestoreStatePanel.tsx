import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { RestoreStageView } from "../../lib/restoreStageView";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";
import { Loader } from "../shared/Loader";

/** A tab or the app restoring: the shared Loader with the restore steps under its caption, or the failure in its place.
 *  Callers already decide when it appears, so it hands over without its own deferral. */
export function RestoreStatePanel({ view, pulse = false }: {
  view: RestoreStageView;
  pulse?: boolean;
}) {
  const currentLabel = view.steps[Math.max(0, view.current)]?.label ?? view.label;
  if (view.phase === "failed") {
    return <BrandState model={brandStateModel({ error: { title: view.label, sentence: view.hint ?? "The session could not be restored." } })} className="ct-loading-material">
      {view.detail && <InlineErrorNotice error={view.detail} operation="restoring this session" />}
    </BrandState>;
  }
  return (
    <Loader label={view.label} detail={view.phase === "sleeping" ? view.hint ?? undefined : undefined} animate={view.phase !== "sleeping"}
      pulse={pulse} deferred={false} surfaceClassName="ct-loading-material">
      {view.phase !== "sleeping" && <div className="ct-loading-progress mx-auto mt-5 w-full max-w-[200px]">
        <div aria-hidden="true" className="restore-progress-labels mb-3 text-xs text-[var(--ct-muted-fg)]">
          {view.steps.map((step, index) => <span key={step.id} data-current={index === Math.max(0, view.current)}>{step.label}</span>)}
        </div>
        <div role="progressbar" aria-label="Restore progress" aria-valuemin={0}
          aria-valuenow={view.phase === "ready" ? view.steps.length : Math.max(0, view.current)}
          aria-valuemax={view.steps.length} aria-valuetext={view.hint ? currentLabel + ". " + view.hint : currentLabel}
          className="flex w-full gap-1">
          {view.steps.map(step => <span key={step.id} aria-hidden="true" data-state={step.state} className="restore-progress-segment"><span /></span>)}
        </div>
        {view.hint && <p className="mt-3 text-xs text-[var(--ct-muted-fg)]">{view.hint}</p>}
      </div>}
    </Loader>
  );
}
