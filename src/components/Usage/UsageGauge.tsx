import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { clampPortalCenter } from "./portalPosition";
import { formatResetsAt } from "./format";
import "./styles.css";

function shortDesc(title: string): string {
  switch (title) {
    case "End of today":
      return "by end of day";
    case "End of cycle":
      return "by end of cycle";
    case "Projection at reset":
      return "by session reset";
    default:
      return "";
  }
}

// ---------- Gauge with surveyor-mark projection markers ----------

type MarkerVariant = "session" | "weekly" | "success" | "warn" | "danger";

export interface GaugeMarker {
  posPct: number; // pinned to right edge if > 100
  valuePct: number;
  variant: MarkerVariant;
  title: string;
  desc: string;
}

interface UsageGaugeProps {
  label: string;
  variant: "session" | "weekly";
  pct: number | null;
  resetsAt: number | null;
  state: "neutral" | "warn" | "danger";
  markers?: GaugeMarker[];
  now: number;
  showIntroHint?: boolean;
}

export function UsageGauge({
  label,
  variant,
  pct,
  resetsAt,
  state,
  markers = [],
  now,
  showIntroHint = false,
}: UsageGaugeProps) {
  const width = pct == null ? 0 : Math.max(0, Math.min(100, pct));
  return (
    <div className={`ct-usage-gauge ${variant} ${state}`}>
      <span className="label">{label}</span>
      <span className="pct">{pct == null ? "—" : `${Math.round(pct)}%`}</span>
      <div className="bar-wrap">
        <div className="bar">
          <div className="fill" style={{ width: `${width}%` }} />
        </div>
        {(() => {
          const pinnedIdx = markers.findIndex((m) => m.posPct > 100);
          const hintIdx = pinnedIdx >= 0 ? pinnedIdx : 0;
          const showLabel = labelVisibility(markers);
          return markers.map((m, i) => (
            <Marker
              key={i}
              marker={m}
              variantDefault={variant}
              introHint={showIntroHint && i === hintIdx ? introFor(m) : undefined}
              showLabel={showLabel[i]}
            />
          ));
        })()}
      </div>
      <span className="reset">{resetsAt ? formatResetsAt(resetsAt, now) : "—"}</span>
    </div>
  );
}

function introFor(m: GaugeMarker): string {
  if (m.posPct > 100) {
    return "at this pace, will overspend by cycle end";
  }
  return `projected ${Math.round(m.valuePct)}% ${shortDesc(m.title)}`;
}

// Hide labels that would collide with earlier markers.
// Earlier means lower posPct (more left); threshold is ~9% of bar width.
function labelVisibility(markers: GaugeMarker[]): boolean[] {
  const MIN_SPACING_PCT = 9;
  const result: boolean[] = markers.map(() => true);
  const sorted = markers
    .map((m, i) => ({ m, i, x: Math.min(100, m.posPct) }))
    .sort((a, b) => a.x - b.x);
  let lastShown = -Infinity;
  for (const item of sorted) {
    if (item.x - lastShown < MIN_SPACING_PCT) {
      result[item.i] = false;
    } else {
      lastShown = item.x;
    }
  }
  return result;
}

interface MarkerProps {
  marker: GaugeMarker;
  variantDefault: "session" | "weekly";
  introHint?: string;
  showLabel?: boolean;
}

function Marker({ marker, variantDefault, introHint, showLabel = true }: MarkerProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [tipPos, setTipPos] = useState<{ x: number; y: number } | null>(null);
  const [introVisible, setIntroVisible] = useState(Boolean(introHint));
  const isPinned = marker.posPct > 100;
  const left = Math.max(0, Math.min(100, marker.posPct));
  const variant = marker.variant === "session" || marker.variant === "weekly" ? variantDefault : marker.variant;

  useEffect(() => {
    if (!introHint) return;
    const t = setTimeout(() => setIntroVisible(false), 4500);
    return () => clearTimeout(t);
  }, [introHint]);

  const show = () => {
    setIntroVisible(false);
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const rawX = r.left + r.width / 2;
    const x = clampPortalCenter(rawX, 240, window.innerWidth);
    setTipPos({ x, y: r.top });
  };
  const hide = () => setTipPos(null);

  return (
    <div
      ref={ref}
      className={`ct-usage-mark ${variant} ${isPinned ? "pinned" : ""}`}
      style={{ left: `${left}%` }}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
      tabIndex={0}
    >
      <span className="dot" />
      {showLabel && !introVisible && !tipPos && marker.posPct <= 100 && (
        <span className="num" aria-hidden>
          {Math.round(marker.valuePct)}%
        </span>
      )}
      {introVisible && introHint && !tipPos && (
        <span className="intro-hint" aria-hidden>
          {introHint}
        </span>
      )}
      {tipPos &&
        createPortal(
          <div className={`ct-usage-mark-tip ${variant} ${isPinned ? "pinned" : ""}`} style={{ left: tipPos.x, top: tipPos.y }} role="tooltip">
            <div className="t-title">{marker.title}</div>
            {!isPinned && (
              <div className="t-num">
                {Math.round(marker.valuePct)}
                <span className="unit">%</span>
              </div>
            )}
            <div className="t-desc">{marker.desc}</div>
          </div>,
          document.body,
        )}
    </div>
  );
}
