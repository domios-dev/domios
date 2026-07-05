import { AlertTriangle, Bot, GitBranch, MonitorSmartphone, Radio } from "lucide-react";
import type { WorkingOnChip, WorkingOnChipKind, WorkingOnRowModel } from "./workingOnRow";

const TONE_CLASS: Record<WorkingOnChip["tone"], string> = {
  info: "working-on-chip-info",
  amber: "working-on-chip-amber",
  red: "working-on-chip-red",
  grey: "working-on-chip-grey",
};

function ChipIcon({ kind }: { kind: WorkingOnChipKind }) {
  const size = 10;
  if (kind === "model") return <Bot size={size} />;
  if (kind === "pane") return <MonitorSmartphone size={size} />;
  if (kind === "peer") return <Radio size={size} />;
  if (kind === "branch") return <GitBranch size={size} />;
  if (kind === "warn") return <AlertTriangle size={size} />;
  return null;
}

export interface WorkingOnRowProps {
  model: WorkingOnRowModel;
  variant?: "board" | "list";
}

export function WorkingOnRow({ model, variant = "board" }: WorkingOnRowProps) {
  if (!model.chips.length) return null;
  return (
    <div data-testid="working-on-row" className={`working-on-row working-on-row-${variant}`}>
      {model.chips.map((chip, i) => (
        <span
          key={`${chip.kind}-${i}`}
          data-testid={`working-on-chip-${chip.kind}`}
          data-tone={chip.tone}
          className={`working-on-chip ${TONE_CLASS[chip.tone]}`}
        >
          <span className="working-on-chip-icon" aria-hidden>
            <ChipIcon kind={chip.kind} />
          </span>
          <span className="min-w-0 truncate">{chip.label}</span>
        </span>
      ))}
    </div>
  );
}
