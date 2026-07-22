import { createPortal } from "react-dom";
import { clampPortalCenter } from "./portalPosition";
import type { Bucket } from "./cycle";

export interface BarHover {
  bucket: Bucket;
  x: number;
  y: number;
  /** Cumulative weekly % at the END of this bucket's day (actuals for past, target for current/future). */
  cumEnd?: number;
  /** True when this is the FIRST bucket where cum crosses 100%. */
  isOverspendStart?: boolean;
  /** True when an earlier bucket already crossed 100% (this one continues over). */
  isAfterOverspend?: boolean;
}

interface Props {
  hover: BarHover;
  showProjection: boolean;
}

export function BarTooltip({ hover, showProjection }: Props) {
  const { bucket: b, x, y, cumEnd, isOverspendStart, isAfterOverspend } = hover;
  const date = new Date(b.start).toLocaleDateString(undefined, {
    weekday: "long",
    month: "short",
    day: "numeric",
  });
  const overspending = isOverspendStart || isAfterOverspend;

  let title: string;
  let body: React.ReactNode;

  if (b.current) {
    const spent = b.pct == null ? 0 : b.pct;
    const plan = b.target;
    const delta = plan == null ? null : spent - plan;
    title = "Today";
    body = (
      <>
        <div>
          spent <b>{Math.round(spent)}%</b> so far
          {plan != null && (
            <>
              {" "}
              · plan <b>{Math.round(plan)}%</b>
            </>
          )}
        </div>
        {delta != null && (
          <div className="ct-usage-tip-foot">
            {delta > 0
              ? `over plan by ${Math.round(delta)}% — future days reduced`
              : delta < 0
                ? `under plan by ${Math.round(-delta)}% — future days raised`
                : "on plan"}
          </div>
        )}
      </>
    );
  } else if (b.isFuture && b.projected && showProjection) {
    title = overspending ? "Overspend" : "Projection";
    body = (
      <>
        <div>
          ~<b>{Math.round(b.target ?? 0)}%</b> this day{b.isWeekend ? " (weekend)" : ""}
        </div>
        {cumEnd != null && (
          <div>
            cumulative <b>{Math.round(cumEnd)}%</b> by end of day
          </div>
        )}
        <div className={`ct-usage-tip-foot ${overspending ? "danger" : ""}`}>
          {isOverspendStart
            ? "at this pace, weekly limit hit on this day"
            : isAfterOverspend
              ? "already past limit on the projected pace"
              : "based on your avg pace"}
        </div>
      </>
    );
  } else if (b.isFuture) {
    title = "Upcoming";
    body = <div>no data yet</div>;
  } else if (b.pct == null) {
    title = "Past day";
    body = <div>no samples recorded</div>;
  } else {
    title = "Past day";
    body = (
      <div>
        spent <b>{Math.round(b.pct)}%</b>
      </div>
    );
  }

  const clampedX = clampPortalCenter(x, 260, window.innerWidth);

  return createPortal(
    <div className="ct-usage-tip" style={{ left: clampedX, top: y }} role="tooltip">
      <div className="ct-usage-tip-title">{title}</div>
      <div className="ct-usage-tip-body">
        <div style={{ marginBottom: 4, opacity: 0.7, fontSize: 10 }}>{date}</div>
        {body}
      </div>
    </div>,
    document.body,
  );
}
