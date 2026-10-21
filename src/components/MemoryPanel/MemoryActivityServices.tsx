import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState } from "react";
import type { AppConfig } from "../../lib/types";
import type { MemNode } from "./types";
import type { useMemoryActivity } from "./useMemoryActivity";

interface Props {
  config?: AppConfig;
  activity: ReturnType<typeof useMemoryActivity>;
  nodes: Map<string, MemNode>;
  onNavigateToItem: (id: string) => void;
}

export function MemoryActivityServices({ config, activity, nodes, onNavigateToItem }: Props) {
  const [filter, setFilter] = useState("");
  const { sync, recent, streamError, events } = activity;
  const filtered = events.filter((event) => !filter || event.op === filter);
  return (
    <section
      role="tabpanel"
      id="activity-services-panel"
      aria-labelledby="activity-services"
      className="space-y-5"
    >
      <div>
        <h2 className="text-sm font-semibold mb-1">Memory services</h2>
        <p className="text-xs text-[var(--ct-muted-fg)]">
          Background maintenance and memory operations.
        </p>
      </div>
      <details className="border border-[var(--ct-border)] rounded-lg p-4 text-xs">
        <summary className="cursor-pointer">
          <span className="text-sm font-medium">Librarian</span>
          <span
            className={`ml-3 ${sync.error || sync.data?.stale_files ? "text-red-400" : "text-[var(--ct-muted-fg)]"}`}
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
        </summary>
        <div className="mt-4 space-y-2 text-[var(--ct-muted-fg)]">
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
      <details className="border border-[var(--ct-border)] rounded-lg p-4 text-xs">
        <summary className="cursor-pointer">
          <span className="text-sm font-medium">Memory operations</span>
          <span
            className={`ml-3 ${recent.error || streamError ? "text-red-400" : "text-[var(--ct-muted-fg)]"}`}
          >
            {recent.loading
              ? "Loading"
              : recent.error || streamError
                ? "Activity unavailable"
                : `${events.length} recent events`}
          </span>
        </summary>
        <div className="mt-4 space-y-3">
          {(recent.error || streamError) && (
            <InlineErrorNotice error={recent.error || streamError} operation="loading recent memory activity" role="alert" />
          )}
          <label className="flex items-center gap-3 text-[var(--ct-muted-fg)]">
            Operation
            <select
              aria-label="Filter memory operations"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              className="border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[var(--ct-fg)] rounded p-1"
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
          <ol className="divide-y divide-[var(--ct-border)]">
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
                    {event.source.kind} / {event.source.agent_provider || "Provider unknown"}
                    {event.source.worker_id && ` / ${event.source.worker_id}`}
                    {event.source.tab_id && ` / ${event.source.tab_id}`}
                  </p>
                </details>
              </li>
            ))}
          </ol>
        </div>
      </details>
    </section>
  );
}
