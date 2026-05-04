import { useState } from "react";
import { useCrons } from "../../store/crons";
import { CronEditor } from "../CronEditor/CronEditor";
import type { CronJob } from "../../lib/types";

export function CronSection({
  workspaceId,
  onOpenCron,
  activeCronId,
}: {
  workspaceId: string;
  onOpenCron?: (jobId?: string) => void;
  activeCronId?: string;
}) {
  const { crons, update, remove } = useCrons();
  const [expanded, setExpanded] = useState(true);
  const [editingCron, setEditingCron] = useState<CronJob | null>(null);

  const wsCrons = crons.filter((c) => c.workspace_id === workspaceId);

  if (wsCrons.length === 0) return null;

  return (
    <>
      <div className="flex items-center mx-2 mt-2 mb-0.5 px-3">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1.5 text-[10px] uppercase tracking-[0.06em] font-medium text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)]"
        >
          <span className="text-[9px]">{expanded ? "▼" : "▶"}</span>
          Periodic Tasks ({wsCrons.length})
        </button>
      </div>

      {expanded &&
        wsCrons.map((job) => (
          <CronItem
            key={job.id}
            job={job}
            onOpen={() => onOpenCron?.(job.id)}
            isActive={activeCronId === job.id}
          />
        ))}

      {editingCron && (
        <CronEditor
          workspaceId={workspaceId}
          existing={editingCron}
          onClose={() => setEditingCron(null)}
          onSave={async (data) => {
            await update(editingCron.id, {
              ...data,
              cwd: data.cwd ?? undefined,
              description: data.description ?? undefined,
            });
            setEditingCron(null);
          }}
          onDelete={async () => {
            await remove(editingCron.id);
            setEditingCron(null);
          }}
        />
      )}
    </>
  );
}

function CronItem({
  job,
  onOpen,
  isActive,
}: {
  job: CronJob;
  onOpen: () => void;
  isActive: boolean;
}) {
  const running = job.last_run?.status === "Running";
  const dotColor = running
    ? "bg-[var(--ct-accent)] animate-pulse"
    : job.enabled
      ? "bg-[var(--ct-green)]"
      : "bg-[var(--ct-muted)]";

  return (
    <div
      className={`group flex items-center gap-1.5 mx-2 px-3 py-1 rounded-md transition-colors cursor-pointer ${
        isActive
          ? "bg-[var(--ct-active)] text-[var(--ct-fg)]"
          : job.enabled
          ? "text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
          : "text-[var(--ct-muted-fg)] opacity-80 hover:bg-[var(--ct-hover)]"
      }`}
      onClick={onOpen}
      style={{ paddingLeft: 20 }}
      title={running ? "Running — click to view live log" : "Click to open task settings"}
    >
      <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${dotColor}`} />
      <span className="text-[11px] truncate flex-1">{job.name}</span>
      {job.enabled && job.next_run && !running && (
        <span className="text-[9px] text-[var(--ct-muted)] shrink-0">
          {new Date(job.next_run).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </span>
      )}
    </div>
  );
}
