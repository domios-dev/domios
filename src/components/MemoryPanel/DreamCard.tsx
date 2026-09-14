import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, Check, Copy, X } from "lucide-react";
import { iconForDream, iconBySlug, paletteFor, moodLabel, parseDream } from "./dreamVisuals";
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
  const palette = paletteFor(`${entry.area ?? ""}:${entry.title}`);
  const mood = parsed.mood ?? moodLabel({ area: entry.area, tags: entry.tags, body: entry.body });
  const date = dreamDate(entry.created_at);
  // Card surfaces the dream's voice (insight). The technical log lives in the modal — never on the card.
  const insight = parsed.insight || entry.body;
  const snippet = insight.length > 200 ? `${insight.slice(0, 200)}…` : insight;
  const visibleTags = (entry.tags ?? []).filter((t) => t !== "dream-diary").slice(0, 3);

  return (
    <article
      data-dream-card
      data-testid="memory-dream-card"
      data-dream-id={entry.id}
      onClick={onOpen}
      className="snap-start shrink-0 w-[260px] h-[280px] rounded-2xl overflow-hidden relative flex flex-col group cursor-pointer transition-[box-shadow] duration-300"
      style={{
        background: palette.surface,
        boxShadow: `0 8px 24px -12px ${palette.glow}, inset 0 0 0 1px ${palette.accent}22`,
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.boxShadow =
          `0 14px 36px -10px ${palette.glow}, 0 0 0 1px ${palette.accent}66, inset 0 0 0 1px ${palette.accent}33`;
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.boxShadow =
          `0 8px 24px -12px ${palette.glow}, inset 0 0 0 1px ${palette.accent}22`;
      }}
    >
      {/* Soft glow blob — extends well outside its visible bounds so any
          rounded-corner clipping is past the falloff and reads as a smooth
          gradient instead of a hard edge. No transforms on the card itself. */}
      <div
        className="absolute -top-10 -right-10 w-44 h-44 rounded-full pointer-events-none opacity-50 group-hover:opacity-80 transition-opacity duration-300"
        style={{
          background: `radial-gradient(circle at center, ${palette.accent} 0%, ${palette.accent}66 40%, transparent 75%)`,
          filter: "blur(28px)",
        }}
      />
      <div className="relative flex items-start justify-between px-4 pt-4">
        <div
          className="w-9 h-9 rounded-lg flex items-center justify-center"
          style={{
            background: `${palette.accent}1f`,
            border: `1px solid ${palette.accent}44`,
            color: palette.accent,
          }}
        >
          <Icon size={16} />
        </div>
        <div className="text-right">
          <div
            className="text-[28px] leading-none font-light tracking-tight"
            style={{ color: palette.ink }}
          >
            {date.day}
          </div>
          <div className="text-[9px] tracking-[0.3em] mt-1 opacity-70" style={{ color: palette.ink }}>
            {date.month}
          </div>
        </div>
      </div>

      <div className="px-4 mt-3 flex items-center gap-1.5 flex-wrap">
        <span
          className="inline-block text-[9px] uppercase tracking-[0.25em] px-2 py-0.5 rounded-full"
          style={{
            background: `${palette.accent}22`,
            color: palette.accent,
          }}
        >
          {mood}
        </span>
        {recordState && recordState.state !== "complete" && (
          <span
            data-testid="memory-dream-card-incomplete"
            data-dream-record-state={recordState.state}
            title={
              recordState.state === "incomplete_live"
                ? `Still being written by ${recordState.workerId}`
                : recordState.reason
            }
            className="inline-block text-[9px] uppercase tracking-[0.2em] px-2 py-0.5 rounded-full"
            style={{
              background: "rgba(251,146,60,0.18)",
              color: "#fb923c",
            }}
          >
            {recordState.state === "incomplete_live" ? "in progress" : "incomplete"}
          </span>
        )}
      </div>

      <div
        className="relative flex-1 px-4 pt-2 pb-3 text-[12px] leading-relaxed overflow-hidden italic"
        style={{ color: palette.ink, opacity: 0.92 }}
      >
        <span className="line-clamp-6">{snippet}</span>
      </div>

      <div className="relative px-4 pb-3 pt-1 border-t border-white/5 flex items-center gap-1.5 flex-wrap">
        {visibleTags.length > 0 ? (
          visibleTags.map((t) => (
            <span
              key={t}
              data-testid="memory-dream-card-tag"
              data-dream-tag={t}
              className="text-[9px] tracking-wide px-1.5 py-0.5 rounded"
              style={{
                background: "rgba(255,255,255,0.06)",
                color: palette.ink,
                opacity: 0.7,
              }}
            >
              #{t}
            </span>
          ))
        ) : (
          <span className="text-[9px] tracking-wide opacity-50" style={{ color: palette.ink }}>
            no tags
          </span>
        )}
      </div>
    </article>
  );
}

export function DreamDetailModal({
  entry,
  onClose,
}: {
  entry: DreamDiaryEntry;
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
  const palette = paletteFor(`${entry.area ?? ""}:${entry.title}`);
  const mood =
    parsed.mood ?? moodLabel({ area: entry.area, tags: entry.tags, body: entry.body });
  const date = dreamDate(entry.created_at);
  const visibleTags = (entry.tags ?? []).filter((t) => t !== "dream-diary");
  const insight = parsed.insight || entry.body;
  const [logOpen, setLogOpen] = useState(false);
  const [copied, setCopied] = useState<"insight" | "all" | null>(null);

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
      setTimeout(() => setCopied(null), 1500);
    } catch { /* ignore */ }
  }, [insight]);

  const copyAll = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(entry.body);
      setCopied("all");
      setTimeout(() => setCopied(null), 1500);
    } catch { /* ignore */ }
  }, [entry.body]);

  return createPortal(
    <div
      onClick={onClose}
      className="fixed inset-0 z-[70] flex items-center justify-center backdrop-blur-md"
      style={{ background: "rgba(2,4,12,0.78)" }}
      data-testid="memory-dream-modal-backdrop"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="relative flex flex-col rounded-2xl overflow-hidden select-text"
        data-testid="memory-dream-modal"
        data-dream-id={entry.id}
        role="dialog"
        aria-label={entry.title}
        style={{
          width: "min(720px, 92vw)",
          height: "min(720px, 88vh)",
          background: palette.surface,
          boxShadow: `0 20px 60px -12px ${palette.glow}, inset 0 0 0 1px ${palette.accent}33`,
          WebkitUserSelect: "text",
          userSelect: "text",
        }}
      >
        <div
          className="absolute top-0 right-0 w-[300px] h-[300px] rounded-full pointer-events-none opacity-50 blur-3xl"
          style={{ background: palette.accent, transform: "translate(40%,-40%)" }}
        />
        <div
          className="absolute bottom-0 left-0 w-[220px] h-[220px] rounded-full pointer-events-none opacity-25 blur-3xl"
          style={{ background: palette.accent, transform: "translate(-40%,40%)" }}
        />

        <div className="relative flex items-start gap-4 px-6 pt-6 pb-3">
          <div
            className="w-12 h-12 rounded-xl flex items-center justify-center shrink-0"
            style={{
              background: `${palette.accent}26`,
              border: `1px solid ${palette.accent}55`,
              color: palette.accent,
            }}
          >
            <Icon size={20} />
          </div>
          <div className="flex-1 min-w-0">
            <span
              className="inline-block text-[10px] uppercase tracking-[0.3em] px-2 py-0.5 rounded-full mb-2"
              style={{
                background: `${palette.accent}26`,
                color: palette.accent,
              }}
            >
              {mood}
            </span>
            <div
              className="text-[15px] leading-tight font-medium"
              style={{ color: palette.ink, opacity: 0.85 }}
            >
              {entry.title}
            </div>
            <div
              className="text-[10px] mt-0.5 opacity-55 tabular-nums"
              style={{ color: palette.ink }}
            >
              {date.month} {date.day}, {date.year}
            </div>
          </div>
          <div className="flex items-start gap-1.5">
            <button
              onClick={copyInsight}
              data-testid="memory-dream-copy-insight"
              aria-label="Copy insight"
              title="Copy insight"
              className="h-7 px-2 rounded-full flex items-center gap-1 bg-white/5 hover:bg-white/10 border border-white/10 cursor-pointer text-[10px] uppercase tracking-wider"
              style={{ color: palette.ink, opacity: 0.85 }}
            >
              {copied === "insight" ? <Check size={12} /> : <Copy size={11} />}
              <span>{copied === "insight" ? "copied" : "copy"}</span>
            </button>
            <button
              onClick={onClose}
              data-testid="memory-dream-modal-close"
              aria-label="Close"
              className="w-7 h-7 rounded-full flex items-center justify-center bg-white/5 hover:bg-white/10 border border-white/10 cursor-pointer"
              style={{ color: palette.ink, opacity: 0.85 }}
            >
              <X size={13} />
            </button>
          </div>
        </div>

        {/* Insight — the dream itself, full bleed serif. */}
        <div
          className="relative flex-1 overflow-y-auto px-6 pt-3 pb-2"
          data-testid="memory-dream-insight"
          style={{ color: palette.ink }}
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

          {parsed.log && (
            <div className="mt-6 pt-4 border-t border-white/10">
              <button
                onClick={() => setLogOpen((v) => !v)}
                data-testid="memory-dream-log-toggle"
                className="flex items-center gap-1.5 text-[10px] uppercase tracking-[0.25em] bg-transparent border-0 cursor-pointer hover:opacity-100"
                style={{ color: palette.ink, opacity: 0.6 }}
              >
                <ChevronDown
                  size={12}
                  className={`transition-transform ${logOpen ? "rotate-0" : "-rotate-90"}`}
                />
                what was done
              </button>
              {logOpen && (
                <div
                  data-testid="memory-dream-log"
                  className="mt-2 px-3 py-2.5 rounded-md font-mono text-[10px] leading-[1.7] whitespace-pre-wrap break-words"
                  style={{
                    background: "rgba(0,0,0,0.25)",
                    color: palette.ink,
                    opacity: 0.78,
                  }}
                >
                  {parsed.log}
                </div>
              )}
            </div>
          )}
        </div>

        <div className="relative px-6 pb-4 pt-3 border-t border-white/8 flex items-center gap-1.5 flex-wrap">
          {visibleTags.length > 0 ? (
            visibleTags.map((t) => (
              <span
                key={t}
                data-testid="memory-dream-modal-tag"
                data-dream-tag={t}
                className="text-[10px] tracking-wide px-2 py-0.5 rounded-full"
                style={{
                  background: "rgba(255,255,255,0.06)",
                  color: palette.ink,
                  opacity: 0.75,
                }}
              >
                #{t}
              </span>
            ))
          ) : (
            <span className="text-[9px] opacity-40" style={{ color: palette.ink }}>
              no tags
            </span>
          )}
          <button
            onClick={copyAll}
            data-testid="memory-dream-copy-raw"
            className="ml-auto h-6 px-2 rounded-full flex items-center gap-1 bg-white/5 hover:bg-white/10 border border-white/10 cursor-pointer text-[9px] uppercase tracking-[0.2em]"
            style={{ color: palette.ink, opacity: 0.7 }}
            title="Copy raw body (insight + log)"
          >
            {copied === "all" ? <Check size={10} /> : <Copy size={10} />}
            <span>{copied === "all" ? "copied" : "raw"}</span>
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
