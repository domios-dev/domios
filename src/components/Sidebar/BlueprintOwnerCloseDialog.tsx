export function BlueprintOwnerCloseDialog({ owner, blueprints, onConfirm, onCancel }: {
  owner: string;
  blueprints: readonly string[];
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return <>
    <div className="fixed inset-0 z-50 bg-black/40" onClick={onCancel} />
    <section role="dialog" aria-modal="true" aria-label={`Close ${owner}`} className="fixed left-1/2 top-1/2 z-50 w-[min(400px,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-2xl">
      <h2 className="px-4 pt-4 text-sm font-semibold">Close {owner}?</h2>
      <p className="px-4 pt-2 text-xs text-[var(--ct-muted-fg)]">These running Blueprints will stop when the owner closes.</p>
      <ul className="max-h-48 overflow-y-auto px-7 py-2 text-xs text-[var(--ct-fg)]">{blueprints.map((blueprint) => <li key={blueprint}>{blueprint}</li>)}</ul>
      <div className="flex justify-end gap-2 border-t border-[var(--ct-border)] px-4 py-3">
        <button type="button" onClick={onCancel} className="rounded-md px-3 py-1.5 text-xs text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)]">Cancel</button>
        <button type="button" onClick={onConfirm} className="rounded-md px-3 py-1.5 text-xs font-medium text-white" style={{ background: "var(--ct-red)" }}>Close {owner}</button>
      </div>
    </section>
  </>;
}
