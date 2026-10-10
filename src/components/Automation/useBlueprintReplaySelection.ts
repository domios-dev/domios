import { useEffect, useMemo, useState } from "react";
import type { BlueprintModel } from "./blueprintModel";
import { blueprintReplayOutcome } from "./blueprintReplayOutcome";
import { replaySteps, type RunRow } from "./blueprintRunModel";

function snapshot(model: BlueprintModel, row: RunRow) {
  return { row, steps: replaySteps(model, row.run), outcome: blueprintReplayOutcome(model, row.run) };
}

interface ReplaySelection {
  key: string;
  nonce: number;
  snapshot?: ReturnType<typeof snapshot>;
}

export function useBlueprintReplaySelection(model: BlueprintModel | null, rows: readonly RunRow[], initialRunId?: string) {
  const [selection, setSelection] = useState<ReplaySelection | null>(initialRunId ? { key: initialRunId, nonce: 0 } : null);
  const row = selection?.snapshot?.row ?? rows.find((candidate) => candidate.key === selection?.key);
  const shown = useMemo(() => selection?.snapshot ?? (model && row ? snapshot(model, row) : null), [selection, model, row]);
  useEffect(() => {
    if (!selection || selection.snapshot || !shown) return;
    setSelection((current) => current?.key === selection.key && current.nonce === selection.nonce ? { ...current, snapshot: shown } : current);
  }, [selection, shown]);
  return {
    selection,
    shown,
    replay: (row: RunRow) => {
      if (model) setSelection((current) => ({ key: row.key, nonce: (current?.nonce ?? 0) + 1, snapshot: snapshot(model, row) }));
    },
    clear: () => setSelection(null),
  };
}
