/**
 * Capability chip, rendered identically across the grant dialog, card, and settings.
 * Variants: granted (active), denied (requested-but-withheld), system (OS permission asked elsewhere).
 */
import type { CSSProperties } from "react";

export type ChipVariant = "granted" | "denied" | "system";

const VARIANT_STYLE: Record<ChipVariant, CSSProperties> = {
  granted: {
    color: "var(--ct-fg)",
  },
  denied: {
    color: "var(--ct-muted)",
  },
  system: {
    color: "var(--ct-fg)",
  },
};

export function CapabilityChip({
  label,
  variant = "granted",
  title,
}: {
  label: string;
  variant?: ChipVariant;
  title?: string;
}) {
  return (
    <span
      title={title}
      data-testid="extensions-capability-chip"
      data-capability-label={label}
      data-capability-variant={variant}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        fontFamily: "inherit",
        fontSize: 12.5,
        lineHeight: 1.4,
        padding: 0,
        whiteSpace: "nowrap",
        ...VARIANT_STYLE[variant],
      }}
    >
      {label}
    </span>
  );
}

/** Human-readable line for a capability id (Chrome-style permission wording). */
export const CAPABILITY_LABELS: Record<string, string> = {
  manage_workspaces: "Create & manage its workspaces",
  spawn_agents: "Spawn AI agents",
  agent_commands: "Receive commands from your agents",
  write_panes: "Type into its panes",
  send_input: "Type into its panes",
  read_panes: "Read its panes",
  read_output: "Read its panes' output",
  read_audio: "Read your voice notes",
  network: "Network & mesh access",
  subprocess: "Run allowed local tools",
  secrets: "Use its own secret store",
  manage_crons: "Schedule recurring tasks",
  admin: "Full control",
  chat_backend: "Drive a chat pane",
  chat_preprocessor: "Augment your messages",
};

export function capabilityLabel(id: string): string {
  return CAPABILITY_LABELS[id] ?? id;
}
