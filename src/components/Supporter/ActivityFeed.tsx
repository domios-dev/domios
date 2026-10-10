import type { ApiConfig } from "../../lib/types";
import { Chip } from "../shared/Chip";
import { UI_TEXT } from "../shared/design";
import { activitySummary, activityText, type SupporterActivity } from "./activity";
import { useSupporterResource } from "./useSupporterResource";

export function ActivityFeed({ api, teamId, embedded = false }: { api: ApiConfig; teamId?: string; embedded?: boolean }) {
  const query = new URLSearchParams({ limit: "50" });
  if (teamId) query.set("team_id", teamId);
  const { data, failed } = useSupporterResource<{ entries: SupporterActivity[] }>(
    api, `/api/supporter/activity?${query}`,
  );
  const entries = (data?.entries ?? []).filter((entry) => !teamId || entry.team_id === teamId);
  return (
    <section
      aria-label="Spinal-cord activity"
      className={`text-xs text-[var(--ct-fg)] ${embedded ? "" : "ct-panel-card p-3"}`}
    >
      {!embedded && <h3 className={`${UI_TEXT.sectionTitle} mb-1`}>Spinal-cord activity</h3>}
      {failed && <p role="status" className={UI_TEXT.detailMuted}>Activity is unavailable.</p>}
      {!data && !failed && <p role="status" className={UI_TEXT.detailMuted}>Loading activity…</p>}
      {data && entries.length === 0 && <p className={UI_TEXT.detailMuted}>No recorded decisions.</p>}
      <ol
        aria-label={teamId ? "Team Spinal Cord decisions" : "Machine Spinal Cord decisions"}
        className="max-h-80 overflow-y-auto"
      >
        {entries.map((entry, index) => {
          const text = activityText(entry);
          const date = new Date(entry.time * 1000);
          const validTime = Number.isFinite(entry.time) && !Number.isNaN(date.getTime());
          const budget = entry.budget_reached === true ? "reached" : entry.budget_reached === false ? "available" : "unknown";
          const attention = entry.fallback === true || text.confidence === "low";
          return (
            <li
              key={`${entry.time}:${index}`}
              data-kind={text.kind}
              data-confidence={text.confidence}
              data-fallback={entry.fallback === true}
            >
              <details className="group">
                <summary className="flex cursor-pointer list-none items-center gap-2 rounded-lg px-1.5 py-1 hover:bg-[color-mix(in_srgb,var(--ct-fg)_5%,transparent)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">
                  <time
                    dateTime={validTime ? date.toISOString() : undefined}
                    title={validTime ? date.toLocaleString() : undefined}
                    className="shrink-0 tabular-nums text-[var(--ct-muted-fg)]"
                  >
                    {validTime ? date.toLocaleTimeString() : "Unknown time"}
                  </time>
                  <span className="min-w-0 flex-1 truncate">{activitySummary(text, entry.asked)}</span>
                  {attention && <Chip variant={entry.fallback === true ? "warning" : "muted"}>{entry.fallback === true ? "fallback" : "low confidence"}</Chip>}
                </summary>
                <div className={`${UI_TEXT.detailMuted} space-y-0.5 pb-2 pl-1`}>
                  {entry.asked && entry.asked.length > 0 && (
                    <dl aria-label="Questions and answers" className="space-y-1 pb-1 text-[var(--ct-fg)]">
                      {entry.asked.map((pair, questionIndex) => (
                        <div key={`${pair.name}:${questionIndex}`}>
                          <dt className="break-words font-medium">{pair.question}</dt>
                          <dd className="break-words">{pair.answer}</dd>
                        </div>
                      ))}
                    </dl>
                  )}
                  <p>{text.kind} · {text.label} · {text.confidence} confidence</p>
                  <p>Action: {text.action}</p>
                  <p>Reason: {text.reason}</p>
                  <p>
                    Budget: {budget} · {entry.fallback === true ? "deterministic fallback" : "no fallback recorded"}
                  </p>
                </div>
              </details>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
