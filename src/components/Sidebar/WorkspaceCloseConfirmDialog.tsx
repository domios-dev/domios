import type { TeamCloseResource } from "./teamCloseResources";

export function WorkspaceCloseConfirmDialog({
  workspaceName,
  agentTitles,
  teams,
  onConfirm,
  onCancel,
}: {
  workspaceName: string;
  agentTitles: string[];
  teams: readonly TeamCloseResource[];
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onCancel} />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Close ${workspaceName}`}
        className="fixed z-50 top-1/2 left-1/2 w-[min(480px,calc(100vw-2rem))] max-h-[80vh] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-2xl"
      >
        <div className="px-4 pt-4 pb-2 text-[13px] font-medium text-[var(--ct-fg)]">
          Close workspace?
        </div>
        <div className="max-h-[60vh] overflow-y-auto px-4 py-1 text-[12px] text-[var(--ct-muted-fg)]">
          <div>Teams ({teams.length})</div>
          {teams.map((team) => (
            <section key={team.id} className="py-2 text-[var(--ct-fg)]">
              <div className="font-medium">{team.name}</div>
              <div className="pt-1 text-[var(--ct-muted-fg)]">Team members ({team.members.length})</div>
              <ul className="pl-3">
                {team.members.map((member) => (
                  <li key={member.tabId}>
                    {member.title}{member.role ? ` (${member.role})` : ""}
                  </li>
                ))}
              </ul>
              <div className="pt-1 text-[var(--ct-muted-fg)]">Crons ({team.crons.length})</div>
              <ul className="pl-3">
                {team.crons.map((cron, index) => <li key={`${cron}-${index}`}>{cron}</li>)}
              </ul>
              <div className="pt-1 text-[var(--ct-muted-fg)]">Lanes ({team.lanes.length})</div>
              <ul className="pl-3">
                {team.lanes.map((lane) => <li key={lane}>{lane}</li>)}
              </ul>
            </section>
          ))}
          <div className="pt-1">Other agent tabs ({agentTitles.length})</div>
          <ul className="px-3 py-1 text-[var(--ct-fg)]">
            {agentTitles.map((title, index) => <li key={`${title}-${index}`}>{title}</li>)}
          </ul>
        </div>
        <div className="flex justify-end gap-2 border-t border-[var(--ct-border)] px-4 py-3">
          <button
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            className="rounded-md px-4 py-1.5 text-[11px] font-medium text-white"
            style={{ background: "var(--ct-red)" }}
          >
            Close workspace
          </button>
        </div>
      </div>
    </>
  );
}
