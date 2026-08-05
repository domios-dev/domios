import { memo, useState } from "react";
import { CheckCircle2, GitCommit } from "lucide-react";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { SidebarDisclosure } from "./SidebarDisclosure";

/** One completed lane, mirroring the Rust `LedgerRow` fields the Done API
 *  returns. Durable: survives the pane closing and vanishing from the sidebar. */
export interface CompletedLane {
  worker_tab_id: string;
  title: string;
  status?: string | null;
  message?: string | null;
  sha?: string | null;
  plan_id?: string | null;
  task_id?: string | null;
  updated_at: number;
}

/** Compact relative age from an `updated_at` unix-SECONDS stamp. */
export function completedAgo(updatedAtSecs: number, nowMs: number = Date.now()): string {
  if (!updatedAtSecs) return "";
  const s = Math.max(0, Math.round(nowMs / 1000 - updatedAtSecs));
  if (s < 45) return "now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d`;
  return new Date(updatedAtSecs * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

/** A landed lane finished with nothing to show (in_review with no SHA) is the
 *  false-completion case the Done view is meant to make visible at a glance. */
export function isMissingLanding(lane: CompletedLane): boolean {
  return !lane.sha && (lane.status ?? "") !== "";
}

interface DoneViewProps {
  lanes: CompletedLane[];
  touchMode?: boolean;
  uiFontScale?: number;
  onSelect?: (lane: CompletedLane) => void;
}

function DoneViewInner({ lanes, touchMode = false, uiFontScale, onSelect }: DoneViewProps) {
  const [open, setOpen] = useState(false);
  const text = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const metaText = pickUiFontScaleValue(uiFontScale, [
    "text-[9px]",
    "text-[10px]",
    "text-[11px]",
    "text-xs",
  ]);
  const rowPad = touchMode ? "py-2" : "py-1.5";

  return (
    <div data-testid="done-view" className="sidebar-done-view">
      <SidebarDisclosure
        expanded={open}
        touchMode={touchMode}
        onClick={() => setOpen((v) => !v)}
        className="sidebar-structural-header w-full"
        title="Completed lanes"
      >
        <span className="sidebar-structural-label truncate">DONE</span>
      </SidebarDisclosure>
      {open && (
        <ul
          aria-label="Completed lanes"
          className="mt-0.5 overflow-y-auto list-none m-0 p-0"
          style={{ maxHeight: 165 }}
        >
          {lanes.length === 0 && (
            <li
              data-testid="done-empty"
              className={`${metaText} px-2 py-1.5 text-[var(--ct-muted)]`}
            >
              Nothing finished yet
            </li>
          )}
          {lanes.map((lane) => {
            const shortSha = lane.sha ? lane.sha.slice(0, 7) : null;
            const statusLabel = lane.status ?? "done";
            const missing = isMissingLanding(lane);
            return (
              <li key={lane.worker_tab_id} data-testid="done-item">
                <button
                  type="button"
                  onClick={onSelect ? () => onSelect(lane) : undefined}
                  aria-label={`${lane.title || "Untitled"} — ${statusLabel}`}
                  className={`flex items-start gap-2 w-full pl-2 pr-2 ${rowPad} rounded-md text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
                >
                  <CheckCircle2
                    size={12}
                    strokeWidth={1.75}
                    className="mt-0.5 shrink-0 text-[var(--ct-accent)]"
                    aria-hidden
                  />
                  <span className="min-w-0 flex-1 text-left">
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span className={`${text} min-w-0 flex-1 truncate leading-tight`}>
                        {lane.title || "Untitled"}
                      </span>
                      <span
                        data-testid="done-status"
                        className={`${metaText} shrink-0 px-1.5 rounded-full border border-[var(--ct-border)] uppercase tabular-nums`}
                      >
                        {statusLabel}
                      </span>
                    </span>
                    <span className="mt-0.5 flex min-w-0 items-center justify-between gap-2 text-[var(--ct-muted)]">
                      {shortSha ? (
                        <span className={`${metaText} inline-flex min-w-0 items-center gap-1`}>
                          <GitCommit size={10} strokeWidth={1.75} aria-hidden />
                          <span className="tabular-nums">{shortSha}</span>
                        </span>
                      ) : missing ? (
                        <span
                          data-testid="done-no-landing"
                          className={`${metaText} text-[var(--ct-warning,#c90)]`}
                        >
                          no commit
                        </span>
                      ) : (
                        <span className="min-w-0 flex-1" />
                      )}
                      {lane.updated_at ? (
                        <span className={`${metaText} shrink-0 tabular-nums text-right`}>
                          {completedAgo(lane.updated_at)}
                        </span>
                      ) : null}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

export const DoneView = memo(DoneViewInner);
