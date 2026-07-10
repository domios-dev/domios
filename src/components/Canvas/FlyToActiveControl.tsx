import { useState } from "react";
import { Crosshair } from "lucide-react";
import type { ActiveWorkEntry } from "./planView/activeWork";
import { displayTaskTitle } from "./planView/activeWork";

export interface FlyToActiveControlProps {
  entries: readonly ActiveWorkEntry[];
  onFly: (taskId: string) => void;
}

export function FlyToActiveControl({ entries, onFly }: FlyToActiveControlProps) {
  const [index, setIndex] = useState(0);
  if (entries.length === 0) return null;

  const entry = entries[index % entries.length]!;
  const cycle = entries.length > 1;
  const title = cycle
    ? `Fly to active work (${index % entries.length + 1}/${entries.length}): ${displayTaskTitle(entry.task)}`
    : `Fly to active: ${displayTaskTitle(entry.task)}`;

  return (
    <button
      type="button"
      data-testid="fly-to-active"
      data-task-id={entry.task.id}
      data-stale={entry.stale ? "true" : "false"}
      className={`ct-fly-to-active${entry.stale ? " ct-fly-to-active-stale" : ""}`}
      title={title}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={() => {
        onFly(entry.task.id);
        if (cycle) setIndex((i) => (i + 1) % entries.length);
      }}
    >
      <Crosshair size={13} aria-hidden />
      <span className="ct-fly-to-active-label">Active</span>
      {cycle && (
        <span className="ct-fly-to-active-cycle" aria-hidden>
          {index % entries.length + 1}/{entries.length}
        </span>
      )}
    </button>
  );
}
