import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection } from "./BoardPanel";
import {
  formatCompact,
  formatCount,
  formatSharePct,
  SPEND_ALLOWANCE_SPENT_NEEDS,
  SPEND_SCOPE_CAPTION,
  SPEND_TOKEN_WORDS,
  SPEND_TOOL_CALLS_NEEDS,
  SPEND_WINDOW_WORDS,
  spendReading,
  type SpendModelRow,
  type SpendProviderGroup,
  type SpendTokens,
  type SpendWindow,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";

function formatWholePct(value: number): string {
  return `${Math.round(value)}%`;
}

const WINDOW_TONE: Record<SpendWindow["state"], string | undefined> = {
  neutral: undefined,
  warn: "mc-board-warn",
  danger: "mc-board-danger",
};

/** Spend reads top-down: allowance used per provider, what each provider took part in, then the model split. */
export function SpendCard({ spend }: { spend: MissionControlSpend }) {
  const reading = spendReading(spend);
  return (
    <BoardPanel
      testId="mc-board-spend"
      title="Spend"
      className="mc-board-spend"
      meta={<span data-testid="mc-board-spend-scope">{SPEND_SCOPE_CAPTION}</span>}
    >
      {reading.emptyCopy ? (
        <BoardPanelSection>
          <p data-testid="mc-board-spend-empty" className="mc-board-note">
            {reading.emptyCopy}
          </p>
        </BoardPanelSection>
      ) : (
        <>
          <BoardPanelSection testId="mc-board-spend-allowance" label="Weekly allowance">
            <div className="mc-board-spend-stack">
              {reading.groups.map((group) => (
                <AllowanceLine key={group.key} group={group} />
              ))}
            </div>
          </BoardPanelSection>
          <BoardPanelSection testId="mc-board-spend-took-part" label="Took part">
            <div className="mc-board-spend-stack">
              {reading.note && (
                <p data-testid="mc-board-spend-units" className="mc-board-caption">
                  {reading.note}
                </p>
              )}
              {reading.groups.map((group) => (
                <TookPartLine key={group.key} group={group} />
              ))}
            </div>
          </BoardPanelSection>
          {reading.models.length > 0 && (
            <BoardPanelSection testId="mc-board-spend-by-model" label="By model">
              <div className="mc-board-spend-stack">
                <ModelStrip models={reading.models} />
                <ModelList models={reading.models} />
              </div>
            </BoardPanelSection>
          )}
        </>
      )}
    </BoardPanel>
  );
}

function Swatch({ tone, unassigned = false }: { tone: number; unassigned?: boolean }) {
  return (
    <span
      aria-hidden="true"
      className="mc-board-swatch"
      data-tone={tone}
      data-unassigned={unassigned ? "true" : undefined}
    />
  );
}

function ProviderName({ group }: { group: SpendProviderGroup }) {
  return (
    <h4 className="mc-board-spend-provider-name">
      <Swatch tone={group.tone} />
      {group.label}
    </h4>
  );
}

function NeedsData({ copy }: { copy: string }) {
  return (
    <span data-testid="mc-board-spend-needs-data" className="mc-board-needs-data">
      {copy}
    </span>
  );
}

function AllowanceLine({ group }: { group: SpendProviderGroup }) {
  return (
    <div data-testid="mc-board-spend-allowance-provider" data-provider={group.key} className="mc-board-spend-line">
      <ProviderName group={group} />
      <span className="mc-board-spend-reading">
        <span className="mc-board-spend-word">spent</span>
        <NeedsData copy={SPEND_ALLOWANCE_SPENT_NEEDS} />
      </span>
      {group.windows.map((window) => (
        <span
          key={window.key}
          data-testid="mc-board-spend-gauge"
          data-window-key={window.key}
          className="mc-board-spend-reading"
        >
          <b data-testid="mc-board-spend-window-pct" className={WINDOW_TONE[window.state]}>
            <AnimatedNumber value={window.pct} format={formatWholePct} />
          </b>
          <span className="mc-board-spend-word">{SPEND_WINDOW_WORDS[window.variant]}</span>
        </span>
      ))}
    </div>
  );
}

function TokenReadings({ tokens, itemClass }: { tokens: SpendTokens; itemClass: string }) {
  return (
    <>
      {SPEND_TOKEN_WORDS.map((entry) => (
        <span key={entry.key} data-figure={entry.key} className={itemClass}>
          <b>
            <AnimatedNumber value={tokens[entry.key]} format={entry.compact ? formatCompact : formatCount} />
          </b>
          <span className="mc-board-spend-word">{tokens[entry.key] === 1 ? entry.one : entry.word}</span>
        </span>
      ))}
    </>
  );
}

function TookPartLine({ group }: { group: SpendProviderGroup }) {
  return (
    <div data-testid="mc-board-spend-took-part-provider" data-provider={group.key} className="mc-board-spend-line">
      <ProviderName group={group} />
      {group.measured ? (
        <TokenReadings tokens={group.tokens} itemClass="mc-board-spend-reading" />
      ) : (
        <span data-testid="mc-board-spend-provider-empty" className="mc-board-spend-reading">
          no data
        </span>
      )}
      <span className="mc-board-spend-reading">
        <span className="mc-board-spend-word">tool calls</span>
        <NeedsData copy={SPEND_TOOL_CALLS_NEEDS} />
      </span>
    </div>
  );
}

/** Decorative: the list below carries every share in words. */
function ModelStrip({ models }: { models: SpendModelRow[] }) {
  return (
    <div data-testid="mc-board-spend-strip" className="mc-board-spend-strip" aria-hidden="true">
      {models.map((model) => (
        <span
          key={model.key}
          className="mc-board-spend-strip-part"
          data-tone={model.tone}
          data-unassigned={model.unassigned ? "true" : undefined}
          style={{ flexGrow: model.sharePct }}
        />
      ))}
    </div>
  );
}

function ModelList({ models }: { models: SpendModelRow[] }) {
  return (
    <ul className="mc-board-spend-models" role="list">
      {models.map((model) => (
        <li
          key={model.key}
          role="listitem"
          data-testid="mc-board-spend-model"
          data-model-key={model.key}
          data-unassigned={model.unassigned ? "true" : "false"}
          className="mc-board-spend-model"
        >
          <span className="mc-board-spend-model-name">
            <Swatch tone={model.tone} unassigned={model.unassigned} />
            {model.label}
          </span>
          <span className="mc-board-spend-model-share">{formatSharePct(model.sharePct)}</span>
          <span className="mc-board-spend-model-figs">
            <TokenReadings tokens={model.tokens} itemClass="mc-board-spend-fig" />
          </span>
        </li>
      ))}
    </ul>
  );
}
