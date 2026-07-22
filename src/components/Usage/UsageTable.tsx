import { useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { classify, type UsageSnapshot } from "../../types/usage";
import { clampPortalCenter } from "./portalPosition";
import { formatRelative } from "./format";

interface UsageTableProps {
  rows: UsageSnapshot[];
  pageSize?: number;
}

export function UsageTable({ rows, pageSize = 5 }: UsageTableProps) {
  const sorted = useMemo(
    () => [...rows].sort((a, b) => b.captured_at - a.captured_at),
    [rows],
  );
  const [visible, setVisible] = useState(pageSize);
  const recent = sorted.slice(0, visible);
  const hasMore = visible < sorted.length;
  const now = Date.now();
  return (
    <div className="ct-usage-table-wrap">
      <table>
        <thead>
          <tr>
            <th style={{ width: "20%" }}>When</th>
            <th style={{ width: "28%" }}>Session</th>
            <th style={{ width: "22%" }}>Weekly</th>
            <th style={{ width: "16%", textAlign: "right" }}>Status</th>
          </tr>
        </thead>
        <tbody>
          {recent.length === 0 && (
            <tr>
              <td colSpan={4} style={{ textAlign: "center", color: "var(--ct-muted)" }}>no samples</td>
            </tr>
          )}
          {recent.map((r) => (
            <TableRow key={r.id} row={r} now={now} />
          ))}
        </tbody>
      </table>
      {hasMore && (
        <div className="ct-usage-loadmore">
          <button
            type="button"
            onClick={() => setVisible((v) => v + pageSize)}
          >
            load more ({sorted.length - visible} remaining)
          </button>
        </div>
      )}
    </div>
  );
}

function TableRow({ row, now }: { row: UsageSnapshot; now: number }) {
  const ok = row.parsed_ok;
  const sessionState = classify(row.session_pct);
  const weeklyState = classify(row.weekly_pct);
  return (
    <>
      <tr>
        <td className="ts">{formatRelative(now, row.captured_at)}</td>
        <NumCell ok={ok} pct={row.session_pct} state={sessionState} />
        <NumCell ok={ok} pct={row.weekly_pct} state={weeklyState} />
        <td style={{ textAlign: "right" }}>
          {ok ? (
            <span className="ct-usage-pill ok">
              <span className="d" />
              ok
            </span>
          ) : (
            <ErrorPill row={row} now={now} />
          )}
        </td>
      </tr>
    </>
  );
}

export function usageFailureCopy(row: UsageSnapshot): {
  providerLabel: string;
  command: string;
  reason: string;
} {
  const providerLabel =
    row.provider === "codex"
      ? "Codex"
      : row.provider === "claude"
        ? "Claude"
        : row.provider || "Provider";
  const command = row.probe_command || (row.provider === "codex" ? "/status" : "/usage");
  const reason = row.failure_reason || "output did not reach a parseable state before the probe timed out";
  return { providerLabel, command, reason };
}

function ErrorPill({ row, now }: { row: UsageSnapshot; now: number }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [tip, setTip] = useState<{ x: number; y: number } | null>(null);
  const copy = usageFailureCopy(row);

  const show = () => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setTip({ x: clampPortalCenter(r.left + r.width / 2, 260, window.innerWidth), y: r.top });
  };
  const hide = () => setTip(null);

  return (
    <>
      <span
        ref={ref}
        className="ct-usage-pill err"
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
        tabIndex={0}
      >
        <span className="d" />
        timeout
      </span>
      {tip &&
        createPortal(
          <div
            className="ct-usage-tip"
            style={{ left: tip.x, top: tip.y }}
            role="tooltip"
          >
            <div className="ct-usage-tip-title">Probe failed</div>
            <div className="ct-usage-tip-body">
              {copy.providerLabel} <code>{copy.command}</code> probe failed.
              <br />
              Reason: {copy.reason}
            </div>
            <div className="ct-usage-tip-foot">{formatRelative(now, row.captured_at)}</div>
          </div>,
          document.body,
        )}
    </>
  );
}

function NumCell({ ok, pct, state }: { ok: boolean; pct: number | null; state: "neutral" | "warn" | "danger" }) {
  if (!ok || pct == null) {
    return <td className="num" style={{ color: "var(--ct-muted)" }}>— —</td>;
  }
  const color = state === "warn" ? "var(--u-warn)" : state === "danger" ? "var(--u-danger)" : undefined;
  return (
    <td className="num">
      <span className="mini">
        <i className={state === "neutral" ? "" : state} style={{ width: `${Math.max(0, Math.min(100, pct))}%` }} />
      </span>
      <span style={color ? { color } : undefined}>{Math.round(pct)}%</span>
    </td>
  );
}
