import type { ApiConfig } from "../../lib/types";
import { UI_TEXT } from "../shared/design";
import type { SupporterActivity } from "./activity";
import { groupReflexes } from "./reflexFeed";
import { useSupporterResource } from "./useSupporterResource";

export function ActivityFeed({ api, teamId, embedded = false }: { api: ApiConfig; teamId?: string; embedded?: boolean }) {
  const query = new URLSearchParams({ limit: "50" });
  if (teamId) query.set("team_id", teamId);
  const { data, failed } = useSupporterResource<{ entries: SupporterActivity[] }>(
    api, `/api/supporter/activity?${query}`,
  );
  const entries = (data?.entries ?? []).filter((entry) => !teamId || entry.team_id === teamId);
  const rows = groupReflexes(entries);
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
        className="ct-reflex-feed"
      >
        {rows.map((row, index) => {
          const date = new Date(row.time * 1000);
          const validTime = Number.isFinite(row.time) && !Number.isNaN(date.getTime());
          const Icon = row.icon;
          const time = validTime ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }) : "—";
          const chip = row.tone === "fallback" ? "fallback" : row.tone === "skipped" ? "skipped" : row.confidence === "low" ? "low confidence" : undefined;
          const accessibleName = `${time} · ${row.phrase}${row.count > 1 ? ` · ×${row.count}` : ""}${chip ? ` · ${chip}` : ""}`;
          return (
            <li
              key={`${row.key}:${index}`}
              className="ct-reflex-row"
              aria-label={accessibleName}
              title={row.phrase}
              data-kind={row.kind}
              data-confidence={row.confidence}
              data-fallback={row.fallback}
              data-tone={row.tone}
            >
              <time dateTime={validTime ? date.toISOString() : undefined} title={validTime ? date.toLocaleString() : undefined}>{time}</time>
              <span className="ct-reflex-row__separator" aria-hidden="true">·</span>
              <Icon size={14} strokeWidth={1.7} aria-hidden="true" />
              <span className="ct-reflex-row__phrase">{row.phrase}</span>
              <span className="ct-reflex-row__count">{row.count > 1 ? `×${row.count}` : ""}</span>
              {chip && <span className="ct-reflex-row__chip">{chip}</span>}
            </li>
          );
        })}
      </ol>
    </section>
  );
}
