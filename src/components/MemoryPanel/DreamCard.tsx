import { useCallback, useEffect, useId, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { Check, Copy, X } from "lucide-react";
import { dreamSummary, iconForDream, iconBySlug, moodLabel, parseDream } from "./dreamVisuals";
import { Button } from "../shared/Button";
import { Chip } from "../shared/Chip";
import type { DiaryRecordState } from "./dreamRunStatus";

export interface DreamDiaryEntry {
  id: string;
  title: string;
  body: string;
  created_at: string;
  area: string | null;
  tags?: string[];
}

function dreamDate(iso: string): { day: string; month: string; year: string } {
  const d = new Date(iso);
  return {
    day: d.toLocaleDateString("en-US", { day: "2-digit" }),
    month: d.toLocaleDateString("en-US", { month: "short" }).toUpperCase(),
    year: d.toLocaleDateString("en-US", { year: "numeric" }),
  };
}

export function DreamCard({
  entry,
  onOpen,
  recordState,
}: {
  entry: DreamDiaryEntry;
  onOpen: () => void;
  recordState?: DiaryRecordState;
}) {
  const parsed = parseDream(entry.body);
  // explicit `<icon>` / `<mood>` block is present.
  const Icon =
    iconBySlug(parsed.icon) ??
    iconForDream({
      area: entry.area,
      tags: entry.tags,
      body: entry.body,
      title: entry.title,
    });
  const mood = parsed.mood ?? moodLabel({ area: entry.area, tags: entry.tags, body: entry.body });
  const date = dreamDate(entry.created_at);
  // Card surfaces the dream's voice (insight). The technical log stays behind the modal's raw copy, never on the card.
  const insight = parsed.insight || entry.body;
  const snippet = insight.length > 200 ? `${insight.slice(0, 200)}…` : insight;
  const visibleTags = (entry.tags ?? []).filter((t) => t !== "dream-diary").slice(0, 3);

  return (
    <article
      data-dream-card
      data-testid="memory-dream-card"
      data-dream-id={entry.id}
      onClick={onOpen}
      className="ct-card snap-start shrink-0 w-[260px] h-[280px] overflow-hidden relative flex flex-col cursor-pointer"
      style={{ "--ct-card-tone": "var(--ct-magenta)" } as CSSProperties}
    >
      <div className="ct-card__head justify-between">
        <div className="ct-card__icon">
          <Icon size={16} />
        </div>
        <div className="text-right ml-auto">
          <div className="text-[28px] leading-none font-light tracking-tight">
            {date.day}
          </div>
          <div className="text-[11px] mt-1 text-[var(--ct-muted-fg)]">
            {date.month}
          </div>
        </div>
      </div>

      <div className="px-4 mt-3 flex items-center justify-center gap-1.5 flex-wrap">
        <Chip variant="muted">{mood}</Chip>
        {recordState && recordState.state !== "complete" && (
          <span
            data-testid="memory-dream-card-incomplete"
            data-dream-record-state={recordState.state}
            title={
              recordState.state === "incomplete_live"
                ? `Still being written by ${recordState.workerId}`
                : recordState.reason
            }
            className="ct-small-pill text-[var(--ct-yellow)]"
          >
            {recordState.state === "incomplete_live" ? "in progress" : "incomplete"}
          </span>
        )}
      </div>

      <div
        data-testid="memory-dream-card-body"
        className="relative flex-1 min-h-0 px-4 pt-2 pb-3 flex items-center justify-center text-center text-[12px] leading-relaxed overflow-hidden italic"
        style={{ color: "var(--ct-fg)", opacity: 0.92 }}
      >
        <span className="line-clamp-6">{snippet}</span>
      </div>

      <div className="relative px-4 pb-3 pt-1 flex items-center justify-center gap-1.5 flex-wrap">
        {visibleTags.length > 0 ? (
          visibleTags.map((t) => (
            <span
              key={t}
              data-testid="memory-dream-card-tag"
              data-dream-tag={t}
              className="ct-small-pill text-[var(--ct-muted-fg)]"
              style={{
                color: "var(--ct-fg)",
                opacity: 0.7,
              }}
            >
              #{t}
            </span>
          ))
        ) : (
          <span className="text-[9px] tracking-wide opacity-50">
            no tags
          </span>
        )}
      </div>
    </article>
  );
}

export function DreamDetailModal({
  entry,
  report,
  onClose,
}: {
  entry: DreamDiaryEntry;
  report?: string | null;
  onClose: () => void;
}) {
  const parsed = parseDream(entry.body);
  const Icon =
    iconBySlug(parsed.icon) ??
    iconForDream({
      area: entry.area,
      tags: entry.tags,
      body: entry.body,
      title: entry.title,
    });
  const mood =
    parsed.mood ?? moodLabel({ area: entry.area, tags: entry.tags, body: entry.body });
  const date = dreamDate(entry.created_at);
  const visibleTags = (entry.tags ?? []).filter((t) => t !== "dream-diary");
  const insight = parsed.insight || entry.body;
  const summary = dreamSummary({ report, log: parsed.log });
  const summaryLabelId = useId();
  const [copied, setCopied] = useState<"insight" | "all" | null>(null);
  const clearCopied = useCallback(() => setCopied(null), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const copyInsight = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(insight);
      setCopied("insight");
    } catch { /* ignore */ }
  }, [insight]);

  const copyAll = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(entry.body);
      setCopied("all");
    } catch { /* ignore */ }
  }, [entry.body]);

  return createPortal(
    <div
      onClick={onClose}
      className="fixed inset-0 z-[70] flex items-center justify-center bg-[color-mix(in_srgb,var(--ct-bg)_70%,transparent)]"
      data-testid="memory-dream-modal-backdrop"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="ct-menu relative flex flex-col overflow-hidden select-text"
        data-testid="memory-dream-modal"
        data-dream-id={entry.id}
        role="dialog"
        aria-label={entry.title}
        style={{
          width: "min(720px, 92vw)",
          height: "min(720px, 88vh)",
          "--ct-card-tone": "var(--ct-magenta)",
          WebkitUserSelect: "text",
          userSelect: "text",
        } as CSSProperties}
      >
        <div className="relative flex flex-wrap items-start gap-3 px-4 sm:px-6 pt-5 pb-3">
          <div
            className="ct-card__icon"
            style={{
              color: "var(--ct-magenta)",
            }}
          >
            <Icon size={20} />
          </div>
          <div className="flex-1 min-w-0">
            <span
              className="ct-small-pill mb-2 text-[var(--ct-muted-fg)]"
              style={{
                color: "var(--ct-magenta)",
              }}
            >
              {mood}
            </span>
            <div
              className="text-[15px] leading-tight font-medium"
              style={{ color: "var(--ct-fg)", opacity: 0.85 }}
            >
              {entry.title}
            </div>
            <div className="text-[10px] mt-0.5 opacity-55 tabular-nums">
              {date.month} {date.day}, {date.year}
            </div>
          </div>
          <div className="ml-auto flex items-start gap-1.5">
            <Button
              onClick={copyInsight}
              onPointerLeave={clearCopied}
              onBlur={clearCopied}
              data-testid="memory-dream-copy-insight"
              aria-label="Copy insight"
              title="Copy insight"
              size="sm"
              style={{ color: "var(--ct-fg)", opacity: 0.85 }}
            >
              {copied === "insight" ? <Check size={12} /> : <Copy size={11} />}
              <span>{copied === "insight" ? "copied" : "copy"}</span>
            </Button>
            <Button
              onClick={onClose}
              data-testid="memory-dream-modal-close"
              aria-label="Close"
              variant="icon"
              size="sm"
              style={{ color: "var(--ct-fg)", opacity: 0.85 }}
            >
              <X size={13} />
            </Button>
          </div>
        </div>

        {/* Insight — the dream itself, full bleed serif. */}
        <div
          className="relative flex-1 min-h-0 overflow-y-auto px-4 sm:px-6 pt-3 pb-2"
          data-testid="memory-dream-insight"
        >
          <div
            className="text-[16px] leading-[1.7] whitespace-pre-wrap break-words font-light"
            style={{
              fontFamily: "'Cormorant Garamond', 'EB Garamond', Georgia, serif",
              opacity: 0.95,
            }}
          >
            {insight || (
              <span className="italic opacity-60">No insight recorded for this run.</span>
            )}
          </div>

          {summary && (
            <section
              aria-labelledby={summaryLabelId}
              className="mt-6 pt-4"
            >
              <div
                id={summaryLabelId}
                className="text-xs font-medium"
                style={{ color: "var(--ct-fg)", opacity: 0.6 }}
              >
                summary
              </div>
              <p
                data-testid="memory-dream-summary"
                data-summary-source={summary.source}
                className="mt-1.5 text-[12px] leading-snug break-words"
                style={{ color: "var(--ct-fg)", opacity: 0.85 }}
              >
                {summary.text}
              </p>
            </section>
          )}
        </div>

        <div className="relative px-4 sm:px-6 pb-4 pt-3 flex items-center gap-1.5 flex-wrap">
          {visibleTags.length > 0 ? (
            visibleTags.map((t) => (
              <span
                key={t}
                data-testid="memory-dream-modal-tag"
                data-dream-tag={t}
                className="ct-small-pill text-[var(--ct-muted-fg)]"
                style={{
                  color: "var(--ct-fg)",
                  opacity: 0.75,
                }}
              >
                #{t}
              </span>
            ))
          ) : (
            <span className="text-[9px] opacity-40">
              no tags
            </span>
          )}
          <Button
            onClick={copyAll}
            onPointerLeave={clearCopied}
            onBlur={clearCopied}
            data-testid="memory-dream-copy-raw"
            aria-label="Copy raw body"
            size="sm"
            className="ml-auto"
            style={{ color: "var(--ct-fg)", opacity: 0.7 }}
            title="Copy raw body (insight + log)"
          >
            {copied === "all" ? <Check size={10} /> : <Copy size={10} />}
            <span>{copied === "all" ? "copied" : "raw"}</span>
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
