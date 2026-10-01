import type { AppConfig } from "../lib/types";
import { renderKbd } from "../components/Settings/shared";
import { resolveHelpShortcut } from "./shortcuts";
import type { HelpBlock, HelpSettingsTarget } from "./types";
import { HelpFigure } from "./figures";
import { AskButton } from "./AskButton";
import { HelpDetails, HelpMeasurement, SettingsLinkButton } from "./HelpExtraBlocks";

export type OpenSettingsTarget = (target: HelpSettingsTarget) => void;

interface HelpBlockViewProps {
  block: HelpBlock;
  config: AppConfig;
  onOpenSettings?: OpenSettingsTarget;
}

export function HelpBlockView({ block, config, onOpenSettings }: HelpBlockViewProps) {
  switch (block.type) {
    case "paragraph":
      return (
        <p className="text-sm text-[var(--ct-fg)] leading-relaxed" data-testid="help-paragraph">
          {block.text}
        </p>
      );
    case "heading":
      return (
        <h4 className="pt-2 text-sm font-semibold text-[var(--ct-fg)]" data-testid="help-heading">
          {block.text}
        </h4>
      );
    case "list": {
      const ListTag = block.ordered ? "ol" : "ul";
      return (
        <ListTag
          className={`${block.ordered ? "list-decimal" : "list-disc"} pl-5 space-y-1.5 text-sm text-[var(--ct-fg)]`}
          data-testid="help-list"
        >
          {block.items.map((item) => (
            <li key={item} className="leading-relaxed">
              {item}
            </li>
          ))}
        </ListTag>
      );
    }
    case "figure":
      return <HelpFigure figureId={block.figureId} />;
    case "tip":
      return (
        <div
          className="rounded-lg border border-[var(--ct-accent)]/30 bg-[color-mix(in_srgb,var(--ct-accent)_10%,transparent)] px-3 py-2.5 text-sm text-[var(--ct-fg)]"
          data-testid="help-tip"
        >
          <span className="font-medium text-[var(--ct-accent)]">Tip: </span>
          {block.text}
        </div>
      );
    case "ask":
      return <AskButton label={block.label} question={block.question} />;
    case "settings-link":
      return onOpenSettings ? (
        <SettingsLinkButton label={block.label} target={block.target} onOpen={onOpenSettings} />
      ) : null;
    case "details":
      return (
        <HelpDetails summary={block.summary}>
          {block.blocks.map((inner, i) => (
            <HelpBlockView key={`${block.summary}-${i}`} block={inner} config={config} onOpenSettings={onOpenSettings} />
          ))}
        </HelpDetails>
      );
    case "measurement":
      return <HelpMeasurement label={block.label} text={block.text} />;
    case "shortcut-table":
      return (
        <div
          className="rounded-xl border border-[var(--ct-border)] overflow-hidden"
          data-testid="help-shortcut-table"
        >
          {block.rows.map((row) => {
            const binding = resolveHelpShortcut(row.source, config);
            return (
              <div
                key={row.label}
                className="flex items-center justify-between gap-3 px-3 py-2 border-b border-[var(--ct-border)] last:border-b-0 bg-[var(--ct-bg)]/30"
                data-testid="help-shortcut-row"
                data-help-shortcut={row.label}
              >
                <span className="text-xs text-[var(--ct-muted-fg)]">{row.label}</span>
                {renderKbd(binding)}
              </div>
            );
          })}
        </div>
      );
    default:
      return null;
  }
}
