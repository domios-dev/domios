import { useMemo } from "react";
import { classify, type UsageSnapshot } from "../../types/usage";
import { formatRelative, formatStampShort } from "./format";

interface UsageTableProps {
  rows: UsageSnapshot[];
  limit?: number;
}

export function UsageTable({ rows, limit = 50 }: UsageTableProps) {
  const recent = useMemo(
    () => [...rows].sort((a, b) => b.captured_at - a.captured_at).slice(0, limit),
    [rows, limit],
  );
  const now = Date.now();
  return (
    <div className="ct-usage-table-wrap">
      <table>
        <thead>
          <tr>
            <th style={{ width: "34%" }}>Timestamp</th>
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
    </div>
  );
}

function TableRow({ row, now }: { row: UsageSnapshot; now: number }) {
  const ok = row.parsed_ok;
  const sessionState = classify(row.session_pct);
  const weeklyState = classify(row.weekly_pct);
  return (
    <tr>
      <td className="ts">
        {formatStampShort(row.captured_at)}
        <span className="rel">{formatRelative(now, row.captured_at)}</span>
      </td>
      <NumCell ok={ok} pct={row.session_pct} state={sessionState} />
      <NumCell ok={ok} pct={row.weekly_pct} state={weeklyState} />
      <td style={{ textAlign: "right" }}>
        {ok ? (
          <span className="ct-usage-pill ok">
            <span className="d" />
            ok
          </span>
        ) : (
          <span className="ct-usage-pill err">
            <span className="d" />
            parse error
          </span>
        )}
      </td>
    </tr>
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
