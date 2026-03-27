import { useState, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { CronJob } from "../../lib/types";

const SCHEDULE_PRESETS = [
  { label: "Every hour", value: "0 * * * *" },
  { label: "Every 6h", value: "0 */6 * * *" },
  { label: "Daily 9 AM", value: "0 9 * * *" },
  { label: "Weekly Mon 9 AM", value: "0 9 * * 1" },
] as const;

function describeSchedule(cron: string): string {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return cron;
  const [min, hour, dom, mon, dow] = parts;

  if (min === "*" && hour === "*") return "Every minute";
  if (hour === "*" && dom === "*" && mon === "*" && dow === "*") {
    if (min === "0") return "Every hour";
    if (min.startsWith("*/")) return `Every ${min.slice(2)} minutes`;
  }
  if (dom === "*" && mon === "*" && dow === "*") {
    if (hour.startsWith("*/")) return `Every ${hour.slice(2)} hours at :${min.padStart(2, "0")}`;
    return `Daily at ${hour}:${min.padStart(2, "0")}`;
  }
  if (dom === "*" && mon === "*" && dow !== "*") {
    const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const day = days[parseInt(dow)] || dow;
    return `Every ${day} at ${hour}:${min.padStart(2, "0")}`;
  }
  return cron;
}

interface CronEditorProps {
  workspaceId: string;
  existing?: CronJob;
  onSave: (data: {
    name: string;
    schedule: string;
    task: string;
    agent: string;
    cwd: string | null;
    description: string | null;
    enabled?: boolean;
  }) => void;
  onClose: () => void;
  onDelete?: () => void;
}

export function CronEditor({ workspaceId: _workspaceId, existing, onSave, onClose, onDelete }: CronEditorProps) {
  const [name, setName] = useState(existing?.name ?? "");
  const [description, setDescription] = useState(existing?.description ?? "");
  const [scheduleMode, setScheduleMode] = useState<"preset" | "custom">(
    existing ? "custom" : "preset",
  );
  const [presetIndex, setPresetIndex] = useState(0);
  const [customSchedule, setCustomSchedule] = useState(existing?.schedule ?? "0 9 * * *");
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [agent] = useState(existing?.agent ?? "claude");
  const [task, setTask] = useState(existing?.task ?? "");
  const [cwd, setCwd] = useState(existing?.cwd ?? "");

  const schedule = scheduleMode === "preset" ? SCHEDULE_PRESETS[presetIndex].value : customSchedule;

  const handleFolderPick = useCallback(async () => {
    const folder = await open({ directory: true, title: "Select working directory" });
    if (folder) setCwd(folder as string);
  }, []);

  const handleSubmit = useCallback(() => {
    if (!name.trim() || !task.trim()) return;
    onSave({
      name: name.trim(),
      schedule,
      task: task.trim(),
      agent,
      cwd: cwd || null,
      description: description || null,
      enabled,
    });
  }, [name, schedule, task, agent, cwd, description, enabled, onSave]);

  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onClose} />
      <div className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-sidebar-bg)] border border-[var(--ct-border)] rounded-xl shadow-2xl w-[860px] h-[640px] max-w-[90vw] max-h-[85vh] flex flex-col overflow-hidden ct-float">
        <div className="px-4 pt-4 pb-2 flex items-center justify-between">
          <div className="text-[13px] font-medium text-[var(--ct-fg)]">
            {existing ? "Edit Cron Job" : "Create Cron Job"}
          </div>
          {existing && (
            <div className="flex items-center gap-2">
              <span className="text-[11px] text-[var(--ct-muted)]">{enabled ? "Enabled" : "Disabled"}</span>
              <button
                onClick={() => setEnabled(!enabled)}
                className={`relative w-9 h-5 rounded-full transition-colors ${enabled ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]/40"}`}
              >
                <span className={`absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform ${enabled ? "left-[18px]" : "left-0.5"}`} />
              </button>
            </div>
          )}
        </div>

        <div className="px-4 py-3 flex flex-col gap-3 flex-1 overflow-y-auto">
          {/* Name */}
          <div>
            <label className="text-[11px] text-[var(--ct-muted)] block mb-1">Name</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Daily code review"
              className="w-full px-2.5 py-1.5 text-[12px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)]"
            />
          </div>

          {/* Description */}
          <div>
            <label className="text-[11px] text-[var(--ct-muted)] block mb-1">Description</label>
            <input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Optional description"
              className="w-full px-2.5 py-1.5 text-[12px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)]"
            />
          </div>

          {/* Schedule */}
          <div>
            <label className="text-[11px] text-[var(--ct-muted)] block mb-1">Schedule</label>
            <div className="flex gap-1.5 flex-wrap mb-2">
              {SCHEDULE_PRESETS.map((p, i) => (
                <button
                  key={p.value}
                  onClick={() => { setScheduleMode("preset"); setPresetIndex(i); }}
                  className={`px-2 py-1 text-[10px] rounded border transition-colors ${
                    scheduleMode === "preset" && presetIndex === i
                      ? "border-[var(--ct-accent)] bg-[var(--ct-accent)]/10 text-[var(--ct-accent)]"
                      : "border-[var(--ct-border)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                  }`}
                >
                  {p.label}
                </button>
              ))}
              <button
                onClick={() => setScheduleMode("custom")}
                className={`px-2 py-1 text-[10px] rounded border transition-colors ${
                  scheduleMode === "custom"
                    ? "border-[var(--ct-accent)] bg-[var(--ct-accent)]/10 text-[var(--ct-accent)]"
                    : "border-[var(--ct-border)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                }`}
              >
                Custom
              </button>
            </div>
            {scheduleMode === "custom" && (
              <input
                value={customSchedule}
                onChange={(e) => setCustomSchedule(e.target.value)}
                placeholder="*/30 * * * *"
                className="w-full px-2.5 py-1.5 text-[12px] font-mono bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)]"
              />
            )}
            <div className="text-[10px] text-[var(--ct-muted)] mt-1">
              {describeSchedule(schedule)}
            </div>
          </div>

          {/* Task */}
          <div>
            <label className="text-[11px] text-[var(--ct-muted)] block mb-1">Task prompt</label>
            <textarea
              value={task}
              onChange={(e) => setTask(e.target.value)}
              placeholder="Review all TODO comments and create a summary..."
              rows={3}
              className="w-full px-2.5 py-1.5 text-[12px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)] resize-y min-h-[60px]"
            />
          </div>

          {/* Agent */}
          <div>
            <label className="text-[11px] text-[var(--ct-muted)] block mb-1">Agent</label>
            <div className="px-2.5 py-1.5 text-[12px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[var(--ct-muted)]">
              Claude
            </div>
          </div>

          {/* CWD */}
          <div>
            <label className="text-[11px] text-[var(--ct-muted)] block mb-1">Working directory</label>
            <div className="flex gap-1.5">
              <input
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                placeholder="Default (current workspace)"
                className="flex-1 px-2.5 py-1.5 text-[12px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)]"
              />
              <button
                onClick={handleFolderPick}
                className="px-2 py-1.5 text-[11px] border border-[var(--ct-border)] rounded hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
              >
                Browse
              </button>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-[var(--ct-border)]">
          <div>
            {existing && onDelete && (
              confirmDelete ? (
                <div className="flex items-center gap-1.5">
                  <button
                    onClick={() => onDelete()}
                    className="px-3 py-1.5 text-[11px] font-medium text-white bg-[var(--ct-red)] rounded-md hover:opacity-90 transition-opacity"
                  >
                    Yes, delete
                  </button>
                  <button
                    onClick={() => setConfirmDelete(false)}
                    className="px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] rounded-md hover:bg-[var(--ct-hover)] transition-colors"
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <button
                  onClick={() => setConfirmDelete(true)}
                  className="px-3 py-1.5 text-[11px] text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10 rounded-md transition-colors"
                >
                  Delete
                </button>
              )
            )}
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              className="px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] rounded-md hover:bg-[var(--ct-hover)] transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleSubmit}
              disabled={!name.trim() || !task.trim()}
              className="px-4 py-1.5 text-[11px] font-medium text-white bg-[var(--ct-accent)] rounded-md hover:opacity-90 transition-opacity disabled:opacity-40"
            >
              {existing ? "Save" : "Create"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
