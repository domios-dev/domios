import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Clock, Moon } from "lucide-react";
import { AgentIcon } from "../AgentIcon";
import type { RestoreStageView } from "../../lib/restoreStageView";

export function RestoreStatePanel({ providerId, view, onRetry, retrying = false }: {
  providerId?: string | null;
  view: RestoreStageView;
  onRetry: () => void;
  retrying?: boolean;
}) {
  const currentLabel = view.steps[Math.max(0, view.current)]?.label ?? view.label;
  return (
    <div className="ct-loading-material flex h-full min-h-0 w-full items-center justify-center overflow-auto px-6 py-10">
      <section role="status" aria-live="polite" aria-label={view.label}
        className="flex w-full max-w-xs flex-col items-center text-center text-[var(--ct-fg)]">
        <div className="ct-loading-icon relative mb-6 flex h-24 w-24 items-center justify-center rounded-3xl border border-[var(--ct-border)] bg-[var(--ct-surface)]">
          <AgentIcon providerId={providerId} className="text-[56px] leading-none" ariaLabel="Session provider icon" />
          <span aria-hidden="true" className="absolute -right-2 -bottom-2 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 text-[var(--ct-muted-fg)]">{view.phase === "sleeping" ? <Moon size={16} /> : <Clock size={16} />}</span>
        </div>
        <h2 className="ct-loading-copy text-base font-medium tracking-tight">{view.label}</h2>
        {view.phase !== "sleeping" && <div className="ct-loading-progress mt-5 w-full">
          <div aria-hidden="true" className="restore-progress-labels mb-3 text-xs text-[var(--ct-muted-fg)]">
            {view.steps.map((step, index) => <span key={step.id} data-current={index === Math.max(0, view.current)}>{step.label}</span>)}
          </div>
          <div role="progressbar" aria-label="Restore progress" aria-valuemin={0}
            aria-valuenow={view.phase === "ready" ? view.steps.length : Math.max(0, view.current)}
            aria-valuemax={view.steps.length} aria-valuetext={currentLabel}
            className="flex w-full gap-1.5">
            {view.steps.map(step => <span key={step.id} aria-hidden="true" data-state={step.state} className="restore-progress-segment"><span /></span>)}
          </div>
        </div>}
        {view.detail && view.phase === "failed" && <InlineErrorNotice error={view.detail} operation="restoring this session" />}
        {view.phase === "failed" && <button type="button" className="ct-btn mt-5 border border-[var(--ct-border)] bg-[var(--ct-surface)] text-xs" disabled={retrying} onClick={onRetry}>Wake again</button>}
      </section>
    </div>
  );
}
