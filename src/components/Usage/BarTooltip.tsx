import { createPortal } from "react-dom";
import type { Bucket } from "./cycle";

export interface BarHover {
  bucket: Bucket;
  x: number;
  y: number;
}

interface Props {
  hover: BarHover;
  showProjection: boolean;
}

export function BarTooltip({ hover, showProjection }: Props) {
  const { bucket: b, x, y } = hover;
  const date = new Date(b.start).toLocaleDateString(undefined, {
    weekday: "long",
    month: "short",
    day: "numeric",
  });

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
    if (b.isWeekend && b.target === 0) {
      title = "Weekend";
      body = <div>skipped — daily budget is spread across weekdays only</div>;
    } else {
      title = "Projection";
      body = (
        <div>
          you can spend <b>{Math.round(b.target ?? 0)}%</b> on this day
        </div>
      );
    }
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

  return createPortal(
    <div className="ct-usage-tip" style={{ left: x, top: y }} role="tooltip">
      <div className="ct-usage-tip-title">{title}</div>
      <div className="ct-usage-tip-body">
        <div style={{ marginBottom: 4, opacity: 0.7, fontSize: 10 }}>{date}</div>
        {body}
      </div>
    </div>,
    document.body,
  );
}
