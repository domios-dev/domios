import type { ReactNode } from "react";
import { Bot, Check, TerminalSquare, Users } from "lucide-react";
import type { NewSessionMode } from "../../lib/newSessionChoice";
import { useShellProfiles } from "../../hooks/useShellProfiles";
import { DropdownSelect, type DropdownSelectOption } from "../shared/DropdownMenu";
import { Button } from "../shared/Button";

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
      <span className="picker-mode-card-desc" title={description}>{description}</span>
    </button>
  );
}

export function ModeCards({
  mode,
  allowedModes,
  onSelect,
  teamDescription = "Orchestrator + Planner",
}: {
  mode: NewSessionMode;
  allowedModes: readonly NewSessionMode[];
  onSelect: (mode: NewSessionMode) => void;
  /** The chosen template, such as "Release train · 3 workers"; the default team reads "Orchestrator + Planner". */
  teamDescription?: string;
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
        description={teamDescription}
        onSelect={() => onSelect("pair")}
        data-testid="picker-mode-orchestration"
        glyphs={
          <span className="picker-mode-glyph-tile">
            <Users size={18} strokeWidth={1.75} />
          </span>
        }
      />}
      {allowedModes.includes("terminal") && <ModeCard
        selected={mode === "terminal"}
        label="Terminal"
        description="Bare shell, start any tool"
        onSelect={() => onSelect("terminal")}
        data-testid="picker-mode-terminal"
        glyphs={
          <span className="picker-mode-glyph-tile">
            <TerminalSquare size={18} strokeWidth={1.75} />
          </span>
        }
      />}
    </div>
  );
}

export function TerminalModePanel({
  shellProfile,
  peerId,
  disabled,
  onShellChange,
}: {
  shellProfile?: string;
  peerId?: string;
  disabled?: boolean;
  onShellChange: (shellProfile?: string) => void;
}) {
  const { profiles: rawProfiles, loading, error, refresh } = useShellProfiles(peerId);
  const profiles = Array.isArray(rawProfiles) ? rawProfiles : [];
  const options: DropdownSelectOption[] = [
    { value: "", label: "Default shell" },
    ...profiles.map((profile) => ({ value: profile.id, label: profile.name })),
  ];
  return (
    <div className="flex flex-col gap-2" data-testid="picker-terminal-panel">
      <p className="m-0 text-[12px] leading-snug text-[var(--ct-muted-fg)]">
        Opens a bare shell tab in this workspace so you can start any CLI or agent manually.
      </p>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center justify-between text-[11px] font-medium text-[var(--ct-muted-fg)]">
          <span>Shell</span>
          <Button
            variant="ghost"
            size="compact"
            disabled={disabled || loading}
            onClick={() => { void refresh(); }}
          >
            {error ? "Retry shell detection" : "Refresh shells"}
          </Button>
        </div>
        <DropdownSelect
          aria-label="Shell profile"
          data-testid="picker-shell-profile"
          disabled={disabled}
          options={options}
          value={shellProfile ?? ""}
          onChange={(value) => onShellChange(value || undefined)}
        />
      </div>
    </div>
  );
}
