import { useEffect, useReducer, useRef, useState } from "react";
import type { ClosedStructureView } from "../../lib/types";
import { closedLabel, undoableClose } from "../../lib/closeHistoryModel";
import { useMultiplexer, useMuxState } from "../../store/multiplexer";

export function CloseUndoAction({ records, restore, now = Date.now }: {
  records: readonly ClosedStructureView[];
  restore: (id: string) => void | Promise<void>;
  now?: () => number;
}) {
  const [, refresh] = useReducer((value) => value + 1, 0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  useEffect(() => {
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, []);
  const record = undoableClose(records, now());
  if (!record) return null;
  const undo = async () => {
    if (pending.current) return;
    const eligible = undoableClose(records, now());
    if (eligible?.id !== record.id) { refresh(); return; }
    pending.current = true;
    setBusy(true);
    setError(null);
    try { await restore(record.id); }
    catch (reason) { setError(String(reason)); }
    finally { pending.current = false; setBusy(false); }
  };
  return <div role="status" className="fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-4 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 text-[var(--ct-fg)] shadow-lg">
    <span>Closed {closedLabel(record).toLowerCase() === "team" ? "a Team" : closedLabel(record)}</span>
    <button type="button" disabled={busy} className="rounded-md px-2 py-0.5 font-medium text-[var(--ct-accent)] hover:bg-[var(--ct-surface-hover,var(--ct-bg))] disabled:opacity-50" onClick={() => { void undo(); }}>Undo</button>
    {error && <span role="alert">{error}</span>}
  </div>;
}

export function CloseUndoToast() {
  const state = useMuxState();
  const { reopenGroup } = useMultiplexer();
  return <CloseUndoAction records={state.closed_structures ?? []} restore={reopenGroup} />;
}
