import type { ReactNode } from "react";
import { Bot, Check, Users } from "lucide-react";
import type { NewSessionMode } from "../../lib/newSessionChoice";

interface ModeCardProps {
  selected: boolean;
  label: string;
  description: string;
  glyphs: ReactNode;
  onSelect: () => void;
  "data-testid": string;
}

function ModeCard({ selected, label, description, glyphs, onSelect, ...rest }: ModeCardProps) {
  return (
    <button
      type="button"
      data-testid={rest["data-testid"]}
      data-selected={selected}
      onClick={onSelect}
      className="picker-mode-card ct-liquid-interactive"
      aria-label={label}
      aria-pressed={selected}
    >
      <span className="picker-mode-card-check" aria-hidden><Check size={12} /></span>
      <span className="picker-mode-card-glyphs" aria-hidden>
        {glyphs}
      </span>
      <span className="picker-mode-card-label">{label}</span>
      <span className="picker-mode-card-desc">{description}</span>
    </button>
  );
}

export function ModeCards({
  mode,
  allowedModes,
  onSelect,
}: {
  mode: NewSessionMode;
  allowedModes: readonly NewSessionMode[];
  onSelect: (mode: NewSessionMode) => void;
}) {
  return (
    <div className="picker-mode-cards">
      <ModeCard
        selected={mode === "single"}
        label="Single agent"
        description="One chat, one agent"
        onSelect={() => onSelect("single")}
        data-testid="picker-mode-single"
        glyphs={
          <span className="picker-mode-glyph-tile">
            <Bot size={18} strokeWidth={1.75} />
          </span>
        }
      />
      {allowedModes.includes("pair") && <ModeCard
        selected={mode === "pair"}
        label="Team"
        description="Orchestrator + Planner"
        onSelect={() => onSelect("pair")}
        data-testid="picker-mode-orchestration"
        glyphs={
          <span className="picker-mode-glyph-tile">
            <Users size={18} strokeWidth={1.75} />
          </span>
        }
      />}
    </div>
  );
}
