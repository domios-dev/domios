import type { CSSProperties } from "react";
import { dreamHeatmap, dreamTotals, type DreamNight } from "./dreamModel";

export function DreamOverview({ nights, now }: { nights: DreamNight[]; now: number }) {
  const cells = dreamHeatmap(nights, now);
  const totals = dreamTotals(nights, now);
  return <section className="dream-card dream-overview" aria-label="Last 18 weeks">
    <div className="dream-heat-wrap"><div className="dream-row dream-small dream-muted dream-heat-heading"><span>Last 18 weeks</span><span className="dream-heat-description">· each square is a night</span><span className="dream-spacer" />
      <span className="dream-row dream-heat-key">Less{[0, 22, 42, 62, 85].map((value) => <i key={value} style={{ "--dream-strength": `${value}%` } as CSSProperties} />)}More</span>
    </div>
    <div className="dream-heat" aria-label="Nightly activity">{cells.map((cell) => {
      const stopped = cell.nights.some((night) => night.state === "stopped");
      const activity = cell.nights.reduce((sum, night) => sum + night.counters.links + night.counters.merged, 0);
      const strength = cell.nights.length ? [22, 42, 62, 85][Math.min(3, Math.floor(activity / 5))] : 0;
      const date = new Date(cell.at).toLocaleDateString("en-US", { month: "short", day: "numeric" });
      return <i key={cell.at} title={`${date}: ${cell.future ? "upcoming" : stopped ? "stopped" : cell.nights.length ? `${cell.nights.length} runs, ${activity} links and merges` : "no run"}`}
        className={[stopped ? "dream-heat-stopped" : "", cell.today ? "dream-heat-today" : "", cell.future ? "dream-heat-future" : ""].join(" ")}
        style={{ "--dream-strength": `${strength}%` } as CSSProperties} />;
    })}</div></div>
    <div className="dream-totals">{[[totals.nights, "nights dreamt"], [totals.links, "links made"], [totals.merged, "duplicates merged"], [totals.stopped, "nights stopped"]].map(([value, label]) =>
      <div key={label}><div className="dream-total">{value}</div><div className="dream-small dream-muted">{label}</div></div>)}</div>
  </section>;
}
