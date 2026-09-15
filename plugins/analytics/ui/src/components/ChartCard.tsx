import { useMemo, useState } from "react";
import type { AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { BoardPanel, BoardPanelSection, Facts } from "../../../../../src/components/Canvas/missionControl/BoardPanel";
import { SpendChart } from "../../../../../src/components/Canvas/missionControl/BoardSpendChart";
import type { SpendChartTone } from "../../../../../src/components/Canvas/missionControl/spendChart";
import { Segmented } from "../../../../../src/components/shared/Segmented";
import { bucketReading, chartData, MEASURE_OPTIONS, measureLabel, type Measure } from "../model/chart";
import { formatCompact, formatCount } from "../model/format";

export function ChartCard({
  snapshot,
  tones,
  labels,
  measure,
  onMeasure,
  nowMs,
  narrow,
}: {
  snapshot: AnalyticsSnapshot;
  tones: ReadonlyMap<string, SpendChartTone>;
  labels: ReadonlyMap<string, string>;
  measure: Measure;
  onMeasure: (measure: Measure) => void;
  nowMs: number;
  narrow: boolean;
}) {
  const [selected, setSelected] = useState<number | null>(null);
  const compact = narrow || snapshot.tokens.series.length > 10;
  const data = useMemo(
    () => chartData(snapshot, measure, tones, labels, nowMs, compact),
    [snapshot, measure, tones, labels, nowMs, compact],
  );
  const unit = snapshot.bucket === "week" ? "week" : "day";
  const format = measure === "turns" ? formatCount : formatCompact;
  const reading = selected === null ? null : bucketReading(data, snapshot, selected, measure);

  return (
    <BoardPanel
      testId="analytics-chart"
      title={`${measureLabel(measure)} by ${unit}`}
      className="analytics-chart-panel"
      meta={
        <Segmented
          value={measure}
          options={MEASURE_OPTIONS}
          onChange={onMeasure}
          aria-label="Measure"
          data-testid="analytics-measure"
          height={narrow ? 36 : 24}
        />
      }
    >
      <BoardPanelSection>
        <SpendChart
          data={data}
          label={measureLabel(measure)}
          unit={unit}
          format={format}
          selectedStart={selected}
          onSelectDay={(start) => setSelected((current) => (current === start ? null : start))}
        />
        <div data-testid="analytics-chart-reading" aria-live="polite" className="analytics-readout">
          {!reading ? (
            <p className="mc-board-caption">Select a bar for its figures.</p>
          ) : reading.value === null ? (
            <p className="mc-board-caption">{reading.label}: not recorded, no figure exists.</p>
          ) : (
            <Facts
              items={[
                { key: "label", node: <b>{reading.label}</b> },
                { key: "value", node: `${measureLabel(measure)} ${format(reading.value)}` },
                ...reading.segments.map((segment) => ({ key: segment.key, node: `${segment.label} ${format(segment.value)}` })),
                ...(measure === "turns" || reading.turns === null ? [] : [{ key: "turns", node: `Turns ${formatCount(reading.turns)}` }]),
              ]}
            />
          )}
        </div>
      </BoardPanelSection>
    </BoardPanel>
  );
}
