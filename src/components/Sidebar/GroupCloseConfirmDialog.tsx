import { BusyState } from "../shared/BusyState";
import type { OrchestrationGroupMember } from "./orchestrationGroupClose";
import { countBySection, summarizeGroupClose } from "./orchestrationSections";

export function GroupCloseConfirmDialog({
  mode = "group",
  orchestratorTitle,
  members,
  crons = [],
  blueprints = [],
  lanes = [],
  loading = false,
  error,
  onRetry,
  onConfirm,
  onCancel,
}: {
  mode?: "group" | "team";
  orchestratorTitle: string;
  members: OrchestrationGroupMember[];
  crons?: readonly string[];
  blueprints?: readonly string[];
  lanes?: readonly string[];
  loading?: boolean;
  error?: string;
  onRetry?: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const bySection = summarizeGroupClose(countBySection(members));
  const isTeam = mode === "team";
  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onCancel} />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={isTeam ? "Close Team" : "Close orchestration group"}
        data-testid="group-close-confirm"
        className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[340px] max-w-[calc(100vw-2rem)] max-h-[85vh] flex flex-col overflow-hidden"
      >
        <div className="px-4 pt-4 pb-2">
          <div className="text-[13px] font-medium text-[var(--ct-fg)]">
            {isTeam ? `Close ${orchestratorTitle}?` : "Close orchestration group?"}
          </div>
        </div>
        <div className="min-h-0 overflow-y-auto">
        <div data-testid="group-close-summary" className="px-4 py-1 text-[12px] text-[var(--ct-muted-fg)]">
          {isTeam
            ? loading ? "All Team tabs will close." : `This closes all ${members.length} Team tabs:`
            : `This closes ${members.length + 1} Tabs — the orchestrator${bySection ? ` + ${bySection}` : ""}:`}
        </div>
        <ul className="px-4 py-2 flex flex-col gap-1 text-[12px] text-[var(--ct-fg)]">
          {!isTeam && <li>{orchestratorTitle} (Orchestrator)</li>}
          {members.map((m) => (
            <li key={m.tabId}>
              {m.title}
              {m.role ? ` (${m.role})` : ""}
            </li>
          ))}
        </ul>
        {loading && <div className="px-4 pb-3"><BusyState inline label="Reading Team inventory" /></div>}
        {error !== undefined && <p className="px-4 pb-3 text-[12px] text-[var(--ct-fg)]" role="alert">{error || "Close inventory unavailable."}</p>}
        {isTeam && !loading && error === undefined && (
          <div className="px-4 pb-3 text-[11px] text-[var(--ct-muted-fg)]">
            <div>Crons ({crons.length})</div>
            {crons.length > 0 && (
              <ul className="py-1 text-[var(--ct-fg)]">
                {crons.map((cron, index) => <li key={`${cron}-${index}`}>{cron}</li>)}
              </ul>
            )}
            <div className="pt-1">Running Blueprints ({blueprints.length})</div>
            {blueprints.length > 0 && <ul className="py-1 text-[var(--ct-fg)]">{blueprints.map((blueprint) => <li key={blueprint}>{blueprint}</li>)}</ul>}
            <div className="pt-1">Lanes ({lanes.length})</div>
            {lanes.length > 0 && (
              <ul className="py-1 text-[var(--ct-fg)]">
                {lanes.map((lane) => <li key={lane}>{lane}</li>)}
              </ul>
            )}
          </div>
        )}
        </div>
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
          <button
            onClick={onCancel}
            className="px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] rounded-md hover:bg-[var(--ct-hover)] transition-colors"
          >
            Cancel
          </button>
          {error !== undefined && onRetry ? <button
            onClick={onRetry}
            className="px-4 py-1.5 text-[11px] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] rounded-md"
          >
            Retry
          </button> : <button
            data-testid="group-close-confirm-button"
            onClick={onConfirm}
            disabled={loading || error !== undefined}
            className="px-4 py-1.5 text-[11px] font-medium text-white rounded-md enabled:hover:opacity-90 disabled:opacity-50 transition-opacity"
            style={{ background: "var(--ct-red)" }}
          >
            {isTeam ? "Close Team" : "Close group"}
          </button>}
        </div>
      </div>
    </>
  );
}
