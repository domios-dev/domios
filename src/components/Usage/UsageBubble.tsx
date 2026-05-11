import { useEffect, useRef, useState } from "react";
import "./styles.css";
import { useUsageSnapshot } from "../../hooks/useUsageSnapshot";
import { classify } from "../../types/usage";
import { UsageDetail } from "./UsageDetail";
import { UsageHistoryModal } from "./UsageHistoryModal";

interface UsageBubbleProps {
  provider?: string;
  accountId?: string | null;
}

const ONE_HOUR_MS = 60 * 60 * 1000;

export function UsageBubble({ provider = "claude", accountId = null }: UsageBubbleProps) {
  const { snapshot, refresh, refetch } = useUsageSnapshot({ provider, accountId });
  const [open, setOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [flipUp, setFlipUp] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (!wrapRef.current) return;
      if (!wrapRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDocClick);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDocClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (!open || !wrapRef.current) return;
    const rect = wrapRef.current.getBoundingClientRect();
    const pane = wrapRef.current.closest("[data-pane-id]") as HTMLElement | null;
    const paneRect = pane?.getBoundingClientRect();
    const room = paneRect ? paneRect.bottom - rect.bottom : window.innerHeight - rect.bottom;
    setFlipUp(room < 310);
  }, [open]);

  if (!snapshot) return null;
  const ageMs = Date.now() - snapshot.captured_at;
  if (!snapshot.parsed_ok && ageMs > ONE_HOUR_MS) return null;

  const sessionState = classify(snapshot.session_pct);
  const weeklyState = classify(snapshot.weekly_pct);
  const worst: "neutral" | "warn" | "danger" =
    sessionState === "danger" || weeklyState === "danger"
      ? "danger"
      : sessionState === "warn" || weeklyState === "warn"
        ? "warn"
        : "neutral";

  const bubbleClass = [
    "ct-usage-bubble",
    worst === "warn" ? "is-warn" : "",
    worst === "danger" ? "is-danger" : "",
    open ? "is-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      ref={wrapRef}
      className="ct-usage"
      style={{ position: "absolute", inset: 0, pointerEvents: "none" }}
    >
      <button
        type="button"
        style={{ pointerEvents: "auto" }}
        className={bubbleClass}
        aria-label={`usage session ${formatPct(snapshot.session_pct)} weekly ${formatPct(snapshot.weekly_pct)}`}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
      >
        <span className={`glyph ${sessionState}`}>
          <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <circle cx="6" cy="6" r="4.6" />
            <path d="M6 3.4V6l1.7 1.1" />
          </svg>
          <span className="pct">{formatPct(snapshot.session_pct)}</span>
        </span>
        <span className="sep" />
        <span className={`glyph ${weeklyState}`}>
          <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="M1 7.2 3 5l2.2 2.2L7.4 4l3.4 3.4" />
          </svg>
          <span className="pct">{formatPct(snapshot.weekly_pct)}</span>
        </span>
      </button>

      {open && (
        <UsageDetail
          snapshot={snapshot}
          flipUp={flipUp}
          onRefresh={async () => {
            await refresh();
            await refetch();
          }}
          onOpenHistory={() => {
            setHistoryOpen(true);
            setOpen(false);
          }}
        />
      )}

      {historyOpen && (
        <UsageHistoryModal
          provider={provider}
          accountId={accountId}
          onClose={() => setHistoryOpen(false)}
        />
      )}
    </div>
  );
}

function formatPct(pct: number | null | undefined): string {
  if (pct == null) return "—";
  return `${Math.round(pct)}%`;
}
