import { memo } from "react";
import { Moon } from "lucide-react";
import {
  cloudMachineCardView,
  type CloudFailurePresentation,
} from "../../lib/cloudMachines/stateModel";
import type { CloudReceipt, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { Button } from "../shared/Button";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineFooter } from "./MachineFooter";
import { MachineName } from "./MachineName";
import { MACHINE_CARD_WIDTH } from "./webGraphLayout";
import "./cloudMachines.css";

export interface CloudMachineCardProps {
  machine: Pick<CloudReceipt, "machine_id" | "name" | "size" | "state" | "rate_per_hour">;
  usage?: CloudUsagePayload | null;
  version?: string | null;
  build?: string | null;
  reachabilityHint?: string;
  selected?: boolean;
  failure?: CloudFailurePresentation | null;
  onClick?: () => void;
  onTopUp?: () => void;
  onFailureAction?: (action: CloudFailurePresentation["action"]) => void;
  onRename?: (name: string) => Promise<boolean>;
  renameUnavailable?: boolean;
}

function CloudMachineCardComponent({
  machine,
  usage,
  version,
  build,
  reachabilityHint,
  selected = false,
  failure = null,
  onClick,
  onTopUp,
  onFailureAction,
  onRename,
  renameUnavailable,
}: CloudMachineCardProps) {
  const view = cloudMachineCardView(machine, usage, Boolean(onTopUp));
  const preparing = view.state === "creating" || view.state === "enrolling";
  const clickable = Boolean(onClick) && !preparing && view.state !== "failed";

  const cardClasses = [
    "ct-card ct-cloud-card relative text-left",
    clickable ? "cursor-pointer" : "ct-card--static",
    view.dimCard ? "ct-cloud-card--asleep" : "",
    preparing ? "ct-cloud-card--preparing" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const borderStyle = selected
    ? {
        width: MACHINE_CARD_WIDTH,
        borderColor: "color-mix(in srgb, var(--ct-accent) 40%, transparent)",
        background: "color-mix(in srgb, var(--ct-accent) 3%, color-mix(in srgb, var(--ct-fg) 2.5%, var(--ct-bg)))",
      }
    : { width: MACHINE_CARD_WIDTH };

  const statusIcon =
    view.statusIcon === "moon" ? (
      <Moon size={12} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
    ) : undefined;

  return (
    <div
      role="group"
      aria-label={`${machine.name}, cloud machine, ${view.listStatus}`}
      data-testid="cloud-machine-card"
      data-cloud-state={view.state}
      onClick={clickable ? onClick : undefined}
      className={cardClasses}
      style={borderStyle}
    >
      <div className="ct-card__head">
        <span
          role={clickable ? "button" : undefined}
          tabIndex={clickable ? 0 : undefined}
          aria-label={clickable ? `Open ${machine.name}, cloud machine, ${view.listStatus}` : undefined}
          onKeyDown={
            clickable
              ? (event) => {
                  if (event.code === "Enter" || event.code === "Space") {
                    event.preventDefault();
                    onClick?.();
                  }
                }
              : undefined
          }
          className="flex items-center gap-2 rounded focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
        >
          <MachineGlyph kind="vm" size="tile" muted={view.tileMuted} />
        </span>
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <span className="min-w-0 flex-1 text-[14px] font-medium text-[var(--ct-fg)]">
            <MachineName name={machine.name} onRename={onRename} renameUnavailable={renameUnavailable} />
          </span>
          <span className="truncate whitespace-nowrap font-mono text-[11px] text-[var(--ct-muted-fg)]">
            {view.sizeMeta}
          </span>
        </div>
      </div>

      {view.blockedCard && (
        <div
          onClick={(event) => event.stopPropagation()}
          className="border-t px-3.5 pb-3 pt-2.5"
          style={{ borderColor: "color-mix(in srgb, var(--ct-border) 55%, transparent)" }}
        >
          <div className="text-[12.5px] font-semibold text-[var(--ct-fg)]">{view.blockedCard.title}</div>
          <p className="m-0 mt-1 text-[11.5px] leading-[1.45] text-[var(--ct-muted-fg)]">
            {view.blockedCard.body}
          </p>
          <div className="mt-2.5 flex items-center justify-between gap-2">
            <span className="text-[11px] tabular-nums text-[var(--ct-muted-fg)]">{view.blockedCard.leftLabel}</span>
            {onTopUp && (
              <Button type="button" size="sm" variant="primary" onClick={onTopUp}>
                {view.blockedCard.ctaLabel}
              </Button>
            )}
          </div>
        </div>
      )}

      {reachabilityHint && !preparing && view.state !== "failed" && (
        <p className="m-0 px-3.5 pb-2 text-[11px] text-[var(--ct-muted-fg)]">{reachabilityHint}</p>
      )}

      <MachineFooter
        label={failure ? failure.message : view.footerLabel}
        secondary={failure ? null : view.footerSecondary}
        tone={failure ? "danger" : view.footerTone}
        connecting={failure ? false : view.connecting}
        statusIcon={failure ? undefined : statusIcon}
        version={view.showVersion ? version : null}
        build={view.showVersion ? build : null}
      >
        {failure && onFailureAction && (() => {
          const label = failure.actionLabel;
          const action = failure.action;
          return (
            <Button
              type="button"
              size="compact"
              variant="primary"
              onClick={(event) => {
                event.stopPropagation();
                onFailureAction(action);
              }}
            >
              {label}
            </Button>
          );
        })()}
      </MachineFooter>
    </div>
  );
}

export const CloudMachineCard = memo(CloudMachineCardComponent);
