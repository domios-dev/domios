export function WorkspaceCloseConfirmDialog({
  workspaceName,
  agentTitles,
  onConfirm,
  onCancel,
}: {
  workspaceName: string;
  agentTitles: string[];
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
        className="fixed z-50 top-1/2 left-1/2 w-[340px] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-2xl"
      >
        <div className="px-4 pt-4 pb-2 text-[13px] font-medium text-[var(--ct-fg)]">
          Close workspace?
        </div>
        <div className="px-4 py-1 text-[12px] text-[var(--ct-muted-fg)]">
          This will destroy {agentTitles.length} agent tab{agentTitles.length === 1 ? "" : "s"}:
        </div>
        <ul className="px-4 py-2 text-[12px] text-[var(--ct-fg)]">
          {agentTitles.map((title, index) => <li key={`${title}-${index}`}>{title}</li>)}
        </ul>
        <div className="flex justify-end gap-2 border-t border-[var(--ct-border)] px-4 py-3">
          <button onClick={onCancel} className="rounded-md px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]">Cancel</button>
          <button onClick={onConfirm} className="rounded-md px-4 py-1.5 text-[11px] font-medium text-white" style={{ background: "var(--ct-red)" }}>Close workspace</button>
        </div>
      </div>
    </>
  );
}
