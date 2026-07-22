import type { ReactNode } from "react";
import { AgentIcon } from "../AgentIcon";
import { FloatingTooltip } from "../shared/FloatingTooltip";
import { DropdownSelect, type DropdownSelectOption } from "../shared/DropdownMenu";
import type { NewSessionMode } from "../../lib/newSessionChoice";
import { Button } from "../shared/Button";

interface ModeCardProps {
  mode: NewSessionMode;
  selected: boolean;
  label: string;
  description: string;
  glyphs: ReactNode;
  onSelect: () => void;
  "data-testid": string;
}

function ModeCard({ mode, selected, label, description, glyphs, onSelect, ...rest }: ModeCardProps) {
  return (
    <button
      type="button"
      data-testid={rest["data-testid"]}
      data-selected={selected}
      onClick={onSelect}
      className="picker-mode-card"
      aria-pressed={selected}
    >
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
  singleProviderId,
  orchestratorProviderId,
  plannerProviderId,
  onSelect,
}: {
  mode: NewSessionMode;
  singleProviderId: string;
  orchestratorProviderId: string;
  plannerProviderId: string;
  onSelect: (mode: NewSessionMode) => void;
}) {
  return (
    <div className="picker-mode-cards">
      <ModeCard
        mode="single"
        selected={mode === "single"}
        label="Single agent"
        description="One chat, one agent"
        onSelect={() => onSelect("single")}
        data-testid="picker-mode-single"
        glyphs={
          <span className="picker-mode-glyph-tile">
            <AgentIcon providerId={singleProviderId} className="text-[16px]" />
          </span>
        }
      />
      <ModeCard
        mode="pair"
        selected={mode === "pair"}
        label="Team"
        description="Orchestrator + Planner, paired"
        onSelect={() => onSelect("pair")}
        data-testid="picker-mode-orchestration"
        glyphs={
          <span className={`picker-mode-glyph-pair ${mode === "pair" ? "picker-mode-glyph-pair--assembled" : ""}`}>
            <span className="picker-mode-glyph-tile">
              <AgentIcon providerId={orchestratorProviderId} className="text-[16px]" />
            </span>
            <span className="picker-mode-glyph-bracket" aria-hidden>⟷</span>
            <span className="picker-mode-glyph-tile">
              <AgentIcon providerId={plannerProviderId} className="text-[16px]" />
            </span>
          </span>
        }
      />
    </div>
  );
}

export function WorkspaceRow({
  workspaces,
  selectedWorkspaceId,
  onSelectWorkspace,
  creatingWorkspace,
  newWorkspaceName,
  onNewWorkspaceNameChange,
  onStartCreateWorkspace,
  onConfirmCreateWorkspace,
  onCancelCreateWorkspace,
  helpText,
}: {
  workspaces: { id: string; name: string }[];
  selectedWorkspaceId: string;
  onSelectWorkspace: (id: string) => void;
  creatingWorkspace: boolean;
  newWorkspaceName: string;
  onNewWorkspaceNameChange: (name: string) => void;
  onStartCreateWorkspace: () => void;
  onConfirmCreateWorkspace: () => void;
  onCancelCreateWorkspace: () => void;
  helpText: string;
}) {
  const selectedName = workspaces.find((w) => w.id === selectedWorkspaceId)?.name ?? "";
  const NEW_WORKSPACE_VALUE = "__new_workspace__";
  const workspaceOptions: DropdownSelectOption[] = [
    ...workspaces.map((w) => ({ value: w.id, label: w.name })),
    { value: NEW_WORKSPACE_VALUE, label: "New workspace…" },
  ];

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-1.5">
        <span className="text-[11px] font-medium text-[var(--ct-muted-fg)]">
          Creates in: {selectedName}
        </span>
        <FloatingTooltip
          content={<div data-testid="workspace-help-tooltip">{helpText}</div>}
          className="picker-help-glyph"
        >
          <span data-testid="workspace-help" className="picker-help-glyph-mark">?</span>
        </FloatingTooltip>
      </div>
      {creatingWorkspace ? (
        <div className="flex items-center gap-1.5">
          <input
            autoFocus
            value={newWorkspaceName}
            onChange={(e) => onNewWorkspaceNameChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onConfirmCreateWorkspace();
              if (e.key === "Escape") onCancelCreateWorkspace();
            }}
            placeholder="Workspace name"
            className="flex-1 min-w-0 px-2 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] text-[12px] text-[var(--ct-fg)] outline-none"
          />
          <Button
            variant="accent"
            size="compact"
            onClick={onConfirmCreateWorkspace}
            disabled={!newWorkspaceName.trim()}
          >
            Create
          </Button>
          <Button variant="ghost" size="compact" onClick={onCancelCreateWorkspace}>
            Cancel
          </Button>
        </div>
      ) : (
        <DropdownSelect
          data-testid="picker-workspace"
          options={workspaceOptions}
          value={selectedWorkspaceId}
          placeholder={selectedName}
          onChange={(value) => {
            if (value === NEW_WORKSPACE_VALUE) {
              onStartCreateWorkspace();
              return;
            }
            onSelectWorkspace(value);
          }}
        />
      )}
    </div>
  );
}
