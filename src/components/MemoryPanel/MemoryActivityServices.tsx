import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Activity, ChevronRight, File } from "lucide-react";
import { useState } from "react";
import type { AppConfig } from "../../lib/types";
import type { MemNode } from "./types";
import type { useMemoryActivity } from "./useMemoryActivity";
import { UI_INPUT } from "../shared/design";

interface Props {
  config?: AppConfig;
  activity: ReturnType<typeof useMemoryActivity>;
  nodes: Map<string, MemNode>;
  onNavigateToItem: (id: string) => void;
}

const serviceRow = "grid cursor-pointer list-none grid-cols-[32px_minmax(0,1fr)_14px] items-start gap-3.5 px-3.5 py-3.5 hover:bg-[var(--ct-bg-hover)] [&::-webkit-details-marker]:hidden";
const serviceChevron = "mt-1 text-[var(--ct-muted-fg)] group-open/service:rotate-90";

export function MemoryActivityServices({ config, activity, nodes, onNavigateToItem }: Props) {
  const [filter, setFilter] = useState("");
  const { sync, recent, streamError, events } = activity;
  const filtered = events.filter((event) => !filter || event.op === filter);
  return (
    <section
      role="tabpanel"
      id="activity-services-panel"
      aria-labelledby="activity-services"
      className="space-y-3.5"
    >
      <div>
        <h2 className="mb-1 text-[13.5px] font-semibold">Memory services</h2>
        <p className="text-[12.5px] text-[var(--ct-muted-fg)]">
          Background maintenance and memory operations.
        </p>
      </div>
      <div className="ct-card ct-card--static divide-y divide-[var(--ct-border-subtle)] overflow-hidden">
        <details className="group/service text-xs">
          <summary className={serviceRow}>
            <span className="ct-card__icon !size-8"><File size={16} aria-hidden="true" /></span>
            <span className="min-w-0">
              <span className="block text-[13.5px] font-semibold">Librarian</span>
              <span
                className={`block text-[12.5px] ${sync.error || sync.data?.stale_files ? "text-[var(--ct-red)]" : "text-[var(--ct-muted-fg)]"}`}
              >
                {sync.loading
                  ? "Loading"
                  : sync.error
                    ? "Status unavailable"
                    : sync.data?.stale_files
                      ? `${sync.data.stale_files} missing source files`
                      : config
                        ? config.memory?.file_sync_enabled
                          ? "Automatic sync configured"
                          : "Automatic sync off"
                        : "Configuration unavailable"}
              </span>
              {sync.data && (
                <span className="mt-2 inline-flex items-center gap-1.5 text-[var(--ct-green)]">
                  <span className="size-1.5 rounded-full bg-current" aria-hidden="true" />
                  {sync.data.tracked_files} files tracked
                </span>
              )}
            </span>
            <ChevronRight size={14} className={serviceChevron} aria-hidden="true" />
          </summary>
          <div className="space-y-2 px-3.5 pb-3.5 text-[var(--ct-muted-fg)]">
            <p>Keeps indexed files and skill folders up to date.</p>
            {sync.data && (
              <p>
                {sync.data.tracked_files} tracked files. {sync.data.stale_files} source files missing.
              </p>
            )}
            <p>
              {config?.memory?.file_sync_enabled
                ? `Configured every ${config.memory.file_sync_interval_minutes} minutes. Runtime health is not reported by this service.`
                : "Enable file synchronization in Memory settings."}
            </p>
            {sync.error && (
              <InlineErrorNotice error={sync.error} operation="syncing memory services" />
            )}
          </div>
        </details>
        <details className="group/service text-xs">
          <summary className={serviceRow}>
            <span className="ct-card__icon !size-8"><Activity size={16} aria-hidden="true" /></span>
            <span className="min-w-0">
              <span className="block text-[13.5px] font-semibold">Memory operations</span>
              <span
                className={`block text-[12.5px] ${recent.error || streamError ? "text-[var(--ct-red)]" : "text-[var(--ct-muted-fg)]"}`}
              >
                {recent.loading
                  ? "Loading"
                  : recent.error || streamError
                    ? "Activity unavailable"
                    : `${events.length} recent events`}
              </span>
            </span>
            <ChevronRight size={14} className={serviceChevron} aria-hidden="true" />
          </summary>
          <div className="space-y-3 px-3.5 pb-3.5">
            {(recent.error || streamError) && (
              <InlineErrorNotice error={recent.error || streamError} operation="loading recent memory activity" role="alert" />
            )}
            <label className="flex items-center gap-3 text-[var(--ct-muted-fg)]">
              Operation
              <select
                aria-label="Filter memory operations"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                className={UI_INPUT.select}
              >
                <option value="">All operations</option>
                {["read", "search", "write", "update", "delete", "recall", "dream"].map((op) => (
                  <option key={op} value={op}>
                    {op}
                  </option>
                ))}
              </select>
            </label>
            {!recent.loading && filtered.length === 0 && (
              <p className="text-[var(--ct-muted-fg)]">
                {recent.error
                  ? "Recent operations could not be loaded."
                  : "No recorded operations in this view."}
              </p>
            )}
            <ol className="divide-y divide-[var(--ct-border-subtle)]">
              {filtered.map((event, index) => (
                <li key={`${event.ts_ms}-${index}`} className="py-3 space-y-2">
                  <div className="flex gap-3">
                    <span>{event.op}</span>
                    <time
                      className="ml-auto text-[var(--ct-muted-fg)]"
                      dateTime={new Date(event.ts_ms).toISOString()}
                    >
                      {new Date(event.ts_ms).toLocaleString()}
                    </time>
                  </div>
                  {event.mem_ids.map((id) => (
                    <button
                      key={id}
                      type="button"
                      className="block text-left text-[var(--ct-fg)] bg-transparent cursor-pointer hover:underline"
                      onClick={() => onNavigateToItem(id)}
                    >
                      {nodes.get(id)?.title || "Open memory"}
                    </button>
                  ))}
                  <details className="text-[var(--ct-muted-fg)]">
                    <summary className="cursor-pointer">Source</summary>
                    <p className="break-all">
                      {event.source.kind} / {event.source.agent_provider || "Harness unknown"}
                      {event.source.worker_id && ` / ${event.source.worker_id}`}
                      {event.source.tab_id && ` / ${event.source.tab_id}`}
                    </p>
                  </details>
                </li>
              ))}
            </ol>
          </div>
        </details>
      </div>
    </section>
  );
}
