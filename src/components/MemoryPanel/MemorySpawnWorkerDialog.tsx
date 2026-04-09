import { memo, useEffect, useState } from "react";
import type { SpawnWorkerReq } from "../../lib/types";

interface Props {
  onSpawn: (req: SpawnWorkerReq) => Promise<void>;
  onClose: () => void;
}

const AGENTS = ["claude"] as const;

function MemorySpawnWorkerDialogImpl({ onSpawn, onClose }: Props) {
  const [intent, setIntent] = useState("");
  const [task, setTask] = useState("");
  const [agent, setAgent] = useState<string>("claude");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, [onClose]);

  const handleSpawn = async () => {
    if (!intent.trim() || !task.trim()) {
      setError("Intent and task are required");
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await onSpawn({
        intent: intent.trim(),
        task: task.trim(),
        agent,
        parent: "user:memory-panel",
      });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const fieldClass =
    "w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] text-[var(--ct-fg)] rounded px-2 py-1.5 text-xs font-[inherit] box-border";
  const labelClass = "block text-[11px] text-[var(--ct-muted-fg)] mb-1";
  const btnClass =
    "bg-transparent border border-[var(--ct-border)] text-inherit px-3.5 py-1.5 rounded text-xs cursor-pointer disabled:opacity-50";

  return (
    <div
      className="absolute inset-0 z-[1050] flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg p-5 w-[480px] max-w-[90%] text-[var(--ct-fg)]"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="m-0 mb-4 text-sm">Spawn Graph Worker</h3>
        <div className="mb-3">
          <label className={labelClass}>Intent</label>
          <input
            className={fieldClass}
            value={intent}
            onChange={(e) => setIntent(e.target.value)}
            placeholder="dedup area=db"
            autoFocus
          />
        </div>
        <div className="mb-3">
          <label className={labelClass}>Task</label>
          <textarea
            className={`${fieldClass} resize-y min-h-[120px]`}
            value={task}
            onChange={(e) => setTask(e.target.value)}
            rows={6}
            placeholder="Full task prompt..."
          />
        </div>
        <div className="mb-3">
          <label className={labelClass}>Agent</label>
          <select
            className={fieldClass}
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
          >
            {AGENTS.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>
        </div>
        <div className="mb-4">
          <label className={labelClass}>Parent</label>
          <input className={fieldClass} value="user:memory-panel" readOnly disabled />
        </div>
        {error && (
          <div className="mb-3 px-2.5 py-1.5 bg-[var(--ct-red)] text-white rounded text-[11px]">
            {error}
          </div>
        )}
        <div className="flex justify-end gap-2">
          <button className={btnClass} onClick={onClose} disabled={submitting}>
            Cancel
          </button>
          <button
            className={`${btnClass} !bg-[var(--ct-accent)] !border-[var(--ct-accent)] !text-white`}
            onClick={handleSpawn}
            disabled={submitting}
          >
            {submitting ? "Spawning…" : "Spawn"}
          </button>
        </div>
      </div>
    </div>
  );
}

export const MemorySpawnWorkerDialog = memo(MemorySpawnWorkerDialogImpl);
