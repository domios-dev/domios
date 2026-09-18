import { useEffect, useState } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { ApiConfig } from "../../../lib/types";
import type { Milestone, Plan } from "../types";
import { fetchPlanSkill, fetchPlanSkills, type PlanSkillSummary } from "./planSkillsApi";
import { skillTimelineForSlug } from "./planRetrospective";
import { milestoneKindLabel, milestoneTimestampLabel } from "./taskActivityModel";
import { PlanFetchErrorState } from "./PlanFetchErrorState";

export interface PlanSkillsSectionProps {
  planId: string;
  plan: Plan;
  api?: ApiConfig;
  compact?: boolean;
  selectedSlug?: string | null;
  onSelectSlug?: (slug: string | null) => void;
  /** Compact header mode: skill click opens the plan document instead of inline detail. */
  onSkillNavigate?: () => void;
}

function CheckBadge({ check }: { check: string | null }) {
  if (!check?.trim()) return null;
  return (
    <span data-testid="plan-skill-check-badge" className="plan-skill-check-badge" title={check}>
      check
    </span>
  );
}

function SkillRow({
  skill,
  selected,
  onSelect,
}: {
  skill: PlanSkillSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      data-testid="plan-skill-row"
      data-skill-slug={skill.slug}
      className={`plan-skill-row${selected ? " is-selected" : ""}`}
      onClick={onSelect}
    >
      <span className="plan-skill-row-title">{skill.title}</span>
      <span className="plan-skill-row-slug">{skill.slug}</span>
      {skill.purpose && <span className="plan-skill-row-purpose">{skill.purpose}</span>}
      <span className="plan-skill-row-verdict">[{skill.verdict}]</span>
      <CheckBadge check={skill.check} />
    </button>
  );
}

function SkillLedgerTimeline({ entries }: { entries: Milestone[] }) {
  if (entries.length === 0) return null;
  const now = Date.now();
  return (
    <div data-testid="plan-skill-ledger-timeline" className="plan-skill-ledger-timeline">
      <div className="plan-skill-ledger-label">Edit ledger</div>
      {entries.map((entry) => (
        <div key={entry.id} data-testid="plan-skill-ledger-item" className="plan-skill-ledger-item">
          <span className="plan-skill-ledger-kind">{milestoneKindLabel(entry.kind)}</span>
          <span className="plan-skill-ledger-age">{milestoneTimestampLabel(entry.ts, now)}</span>
          <span className="plan-skill-ledger-text">{entry.text}</span>
        </div>
      ))}
    </div>
  );
}

function SkillDetailPanel({
  planId,
  plan,
  api,
  slug,
}: {
  planId: string;
  plan: Plan;
  api?: ApiConfig;
  slug: string;
}) {
  const [body, setBody] = useState<string>("");
  const [title, setTitle] = useState(slug);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!api) return;
    let cancelled = false;
    const ctrl = new AbortController();
    fetchPlanSkill(api, planId, slug, ctrl.signal)
      .then((detail) => {
        if (cancelled) return;
        setBody(detail.body);
        setTitle(detail.title);
        setError(null);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
      ctrl.abort();
    };
  }, [api?.host, api?.port, api?.token, planId, slug]);

  const timeline = skillTimelineForSlug(plan, slug);

  return (
    <div data-testid="plan-skill-detail" className="plan-skill-detail">
      <div className="plan-skill-detail-header">
        <strong>{title}</strong>
        <span className="plan-skill-detail-slug">{slug}</span>
      </div>
      {error && <p className="plan-skill-detail-error">{error}</p>}
      {body && (
        <div data-testid="plan-skill-markdown" className="plan-skill-markdown">
          <RichMarkdown text={body} />
        </div>
      )}
      <SkillLedgerTimeline entries={timeline} />
    </div>
  );
}

export function PlanSkillsSection({
  planId,
  plan,
  api,
  compact = false,
  selectedSlug,
  onSelectSlug,
  onSkillNavigate,
}: PlanSkillsSectionProps) {
  const [skills, setSkills] = useState<PlanSkillSummary[]>([]);
  const [loadFailed, setLoadFailed] = useState(false);
  const [reloadNonce, setReloadNonce] = useState(0);
  const [internalSlug, setInternalSlug] = useState<string | null>(null);
  const activeSlug = selectedSlug ?? internalSlug;

  useEffect(() => {
    if (!api) return;
    let cancelled = false;
    const ctrl = new AbortController();
    fetchPlanSkills(api, planId, ctrl.signal)
      .then((rows) => {
        if (cancelled) return;
        setSkills(rows);
        setLoadFailed(false);
      })
      .catch(() => {
        if (cancelled) return;
        setLoadFailed(true);
        setSkills([]);
      });
    return () => {
      cancelled = true;
      ctrl.abort();
    };
  }, [api?.host, api?.port, api?.token, planId, reloadNonce]);

  const pick = (slug: string | null) => {
    if (onSelectSlug) onSelectSlug(slug);
    else setInternalSlug(slug);
  };

  if (!api) return null;
  if (loadFailed) {
    return (
      <PlanFetchErrorState
        loading={false}
        error="Couldn't load plan skills."
        itemCount={0}
        onRetry={() => setReloadNonce((n) => n + 1)}
        testId="plan-skills-error"
        className="plan-skills-error-state"
      />
    );
  }
  if (skills.length === 0 && compact) return null;

  return (
    <section data-testid="plan-skills-section" className={`plan-skills-section${compact ? " is-compact" : ""}`}>
      <div className="plan-skills-label">Plan skills</div>
      {skills.length === 0 ? (
        <p data-testid="plan-skills-empty" className="plan-skills-empty">
          No plan skills yet.
        </p>
      ) : (
        <div className="plan-skills-list">
          {skills.map((skill) => (
            <SkillRow
              key={skill.slug}
              skill={skill}
              selected={activeSlug === skill.slug}
              onSelect={() => {
                if (compact && onSkillNavigate) {
                  onSkillNavigate();
                  return;
                }
                pick(activeSlug === skill.slug ? null : skill.slug);
              }}
            />
          ))}
        </div>
      )}
      {activeSlug && (
        <SkillDetailPanel planId={planId} plan={plan} api={api} slug={activeSlug} />
      )}
    </section>
  );
}
