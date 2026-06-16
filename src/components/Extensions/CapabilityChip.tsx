/**
 * Capability chip — the signature element of the Extensions surface. A plugin is
 * defined by what it's allowed to do, so its capabilities render identically in the
 * install grant dialog, on its card, and in its settings panel.
 *
 * Variants:
 *   granted  — accent-tinted, the permission is active
 *   denied   — hollow/dashed, requested-but-withheld (or not needed)
 *   system   — green-tinted, an OS/device permission asked elsewhere
 */
import type { CSSProperties } from "react";

export type ChipVariant = "granted" | "denied" | "system";

const MONO =
  '"JetBrains Mono", "Cascadia Code", ui-monospace, monospace';

const VARIANT_STYLE: Record<ChipVariant, CSSProperties> = {
  granted: {
    background: "rgba(99,102,241,0.14)",
    color: "#c7c9ff",
    border: "1px solid rgba(99,102,241,0.45)",
  },
  denied: {
    background: "transparent",
    color: "var(--ct-muted, rgba(255,255,255,0.36))",
    border: "1px dashed rgba(255,255,255,0.18)",
  },
  system: {
    background: "rgba(34,197,94,0.10)",
    color: "#86efac",
    border: "1px solid rgba(34,197,94,0.30)",
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
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        fontFamily: MONO,
        fontSize: 10.5,
        lineHeight: 1.4,
        padding: "3px 8px",
        borderRadius: 4,
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
};

export function capabilityLabel(id: string): string {
  return CAPABILITY_LABELS[id] ?? id;
}
