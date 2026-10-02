import type { ApiConfig } from "../../lib/types";
import { activityText, type SupporterActivity } from "./activity";
import { useSupporterResource } from "./useSupporterResource";

export function ActivityFeed({ api, teamId }: { api: ApiConfig; teamId?: string }) {
  const query = new URLSearchParams({ limit: "50" });
  if (teamId) query.set("team_id", teamId);
  const { data, failed } = useSupporterResource<{ entries: SupporterActivity[] }>(
    api, `/api/supporter/activity?${query}`,
  );
  const entries = (data?.entries ?? []).filter((entry) => !teamId || entry.team_id === teamId);
  return (
    <section
      aria-label="Spinal-cord activity"
      className="rounded border border-[var(--ct-border)] p-3 text-xs text-[var(--ct-fg)]"
    >
      <h3 className="mb-2 font-semibold">Spinal-cord activity</h3>
      {failed && <p role="status">Activity is unavailable.</p>}
      {!data && !failed && <p role="status">Loading activity…</p>}
      {data && entries.length === 0 && <p>No recorded decisions.</p>}
      <ol
        aria-label={teamId ? "Team Supporter decisions" : "Machine Supporter decisions"}
        className="max-h-80 overflow-y-auto space-y-3"
      >
        {entries.map((entry, index) => {
          const text = activityText(entry);
          const date = new Date(entry.time * 1000);
          const validTime = Number.isFinite(entry.time) && !Number.isNaN(date.getTime());
          const budget = entry.budget_reached === true ? "reached" : entry.budget_reached === false ? "available" : "unknown";
          return (
            <li
              key={`${entry.time}:${index}`}
              data-kind={text.kind}
              data-confidence={text.confidence}
              data-fallback={entry.fallback === true}
              className="border-b border-[var(--ct-border)] pb-2"
            >
              <time dateTime={validTime ? date.toISOString() : undefined}>
                {validTime ? date.toLocaleString() : "Unknown time"}
              </time>
              <p>{text.kind} · {text.label} · {text.confidence} confidence</p>
              <p>Action: {text.action}</p>
              <p>Reason: {text.reason}</p>
              <p>
                Budget: {budget} · {entry.fallback === true ? "deterministic fallback" : "no fallback recorded"}
              </p>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
