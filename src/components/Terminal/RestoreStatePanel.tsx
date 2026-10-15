import { Check, Clock, Moon } from "lucide-react";
import { AgentIcon } from "../AgentIcon";
import type { RestoreStageView } from "../../lib/restoreStageView";

export function RestoreStatePanel({ providerId, view, onRetry, retrying = false }: {
  providerId?: string | null;
  view: RestoreStageView;
  onRetry: () => void;
  retrying?: boolean;
}) {
  return (
    <div className="flex h-full min-h-0 w-full items-center justify-center overflow-auto bg-[var(--ct-bg)] px-6 py-10">
      <section role="status" aria-live="polite" aria-label={view.label}
        className="flex w-full max-w-xs flex-col items-center text-center text-[var(--ct-fg)]">
        <div className="relative mb-6 flex h-24 w-24 items-center justify-center rounded-3xl border border-[var(--ct-border)] bg-[var(--ct-surface)]">
          <AgentIcon providerId={providerId} className="text-[56px] leading-none" ariaLabel="Session provider icon" />
          <span aria-hidden="true" className="absolute -left-3 bottom-2 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 text-[var(--ct-muted-fg)]"><Moon size={16} /></span>
          <span aria-hidden="true" className="absolute -right-3 top-2 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 text-[var(--ct-muted-fg)]"><Clock size={16} /></span>
        </div>
        <h2 className="text-base font-medium tracking-tight">{view.label}</h2>
        {view.phase !== "sleeping" && <ol aria-label="Restore progress" className="mt-5 w-full space-y-3 text-left text-xs">
          {view.steps.map((step, index) => <li key={step.id} aria-current={step.state === "current" ? "step" : undefined}
            className={`flex items-center gap-3 ${step.state === "pending" ? "text-[var(--ct-muted-fg)] opacity-50" : "text-[var(--ct-fg)]"}`}>
            <span aria-hidden="true" className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full border ${step.state === "current" ? "border-[var(--ct-accent)] text-[var(--ct-accent)]" : "border-[var(--ct-border)]"}`}>
              {step.state === "complete" ? <Check size={11} /> : step.state === "current" ? <Clock size={10} /> : index + 1}
            </span>
            <span>{step.label}</span>
            <span className="sr-only">{step.state}</span>
          </li>)}
        </ol>}
        {view.detail && <p className={`mt-4 max-w-full break-words text-xs leading-relaxed ${view.phase === "failed" ? "text-[var(--ct-danger)]" : "text-[var(--ct-muted-fg)]"}`}>{view.detail}</p>}
        {view.phase === "failed" && <button type="button" className="ct-btn mt-5 border border-[var(--ct-border)] bg-[var(--ct-surface)] text-xs" disabled={retrying} onClick={onRetry}>Wake again</button>}
      </section>
    </div>
  );
}
