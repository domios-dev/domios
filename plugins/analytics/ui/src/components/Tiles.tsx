import type { AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { AnimatedNumber } from "../../../../../src/components/Canvas/missionControl/AnimatedNumber";
import { BoardPanel, BoardPanelSection } from "../../../../../src/components/Canvas/missionControl/BoardPanel";
import { formatCompact, formatCount, formatStamp } from "../model/format";

interface Tile {
  key: string;
  title: string;
  value: number | null;
  format: (value: number) => string;
  caption: string;
  meta?: string;
}

function tabsTile(snapshot: AnalyticsSnapshot, unavailable: boolean): Tile {
  const block = snapshot.tabs_created;
  const base = { key: "tabs", title: "Tabs created", format: formatCount };
  if (unavailable) return { ...base, value: null, caption: "unavailable" };
  if (block.state === "not_attributed") {
    return { ...base, value: null, caption: "A tab carries only its workspace, so the other filters can't apply to it" };
  }
  if (block.state !== "counted") {
    return { ...base, value: null, meta: "not recorded", caption: "Tab creation isn't counted on this machine yet" };
  }
  if (block.total === null) return { ...base, value: null, caption: "No counted day in this range" };
  return {
    ...base,
    value: block.total,
    caption: `${formatCount(block.by_kind?.agent ?? 0)} agent · ${formatCount(block.by_kind?.terminal ?? 0)} terminal${
      block.counted_since !== null ? ` · since ${formatStamp(block.counted_since)}` : ""
    }`,
  };
}

export function TotalsTiles({ snapshot, unavailable }: { snapshot: AnalyticsSnapshot; unavailable: boolean }) {
  const totals = unavailable ? null : snapshot.tokens.totals;
  const missing = unavailable ? "unavailable" : "not recorded";
  const tiles: Tile[] = [
    { key: "new-input", title: "New input", value: totals?.new_input ?? null, format: formatCompact, caption: "uncached input, the part that costs" },
    { key: "generated", title: "Generated", value: totals?.generated ?? null, format: formatCompact, caption: "output tokens" },
    { key: "cache", title: "Read from cache", value: totals?.read_from_cache ?? null, format: formatCompact, caption: "input served from the provider's cache" },
    { key: "turns", title: "Turns", value: totals?.turns ?? null, format: formatCount, caption: "model turns with token counts" },
    tabsTile(snapshot, unavailable),
  ];
  return (
    <ul className="analytics-tiles" role="list">
      {tiles.map((tile) => (
        <li key={tile.key}>
          <BoardPanel testId={`analytics-total-${tile.key}`} title={tile.title} meta={tile.meta}>
            <BoardPanelSection>
              <b data-testid={`analytics-total-${tile.key}-value`} className={`analytics-amount${tile.value === null ? " is-missing" : ""}`}>
                <AnimatedNumber value={tile.value} format={tile.format} />
              </b>
              <p className="mc-board-caption">{tile.value === null && tile.key !== "tabs" ? missing : tile.caption}</p>
            </BoardPanelSection>
          </BoardPanel>
        </li>
      ))}
    </ul>
  );
}
