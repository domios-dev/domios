/**
 * Capability chip, rendered identically across the grant dialog, card, and settings.
 * Variants: granted (active), denied (requested-but-withheld), system (declared plugin capability).
 */
import { Chip, type ChipVariant as SharedChipVariant } from "../shared/Chip";

export type ChipVariant = "granted" | "denied" | "system";

const SHARED_VARIANT: Record<ChipVariant, SharedChipVariant> = {
  granted: "muted",
  denied: "muted",
  system: "default",
};

const VARIANT_CLASS: Record<ChipVariant, string> = {
  granted: "bg-[color-mix(in_srgb,var(--ct-fg)_2.5%,transparent)] text-[var(--ct-muted-fg)]",
  denied: "opacity-60 text-[var(--ct-muted)]",
  system: "bg-[color-mix(in_srgb,var(--ct-fg)_4%,transparent)] text-[var(--ct-fg)]",
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
    <Chip
      variant={SHARED_VARIANT[variant]}
      title={title}
      data-testid="extensions-capability-chip"
      data-capability-label={label}
      data-capability-variant={variant}
      className={`px-2.5 py-0.5 text-[11.5px] font-normal whitespace-nowrap ${VARIANT_CLASS[variant]}`}
    >
      {label}
    </Chip>
  );
}

/** Human sentence-case labels for declared plugin capability flags. */
export const PLUGIN_CAPABILITY_LABELS: Record<string, string> = {
  agent_provider: "Agent harness",
  secret_backend: "Secret store",
  transcriber: "Live transcription",
  glance_view: "Glance panel",
  status_bubble: "Status indicator",
  view: "Plugin view",
  chat_backend: "Chat backend",
  chat_preprocessor: "Message preprocessor",
  graph_nodes: "Blueprint steps",
  local_recording_trigger: "Recording trigger",
};

export function pluginCapabilityLabel(id: string): string {
  const known = PLUGIN_CAPABILITY_LABELS[id];
  if (known) return known;
  const words = id.replace(/_/g, " ").trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : id;
}

/** Concise sentence-case labels for granted permission chips in plugin details. */
export const PERMISSION_CHIP_LABELS: Record<string, string> = {
  local_recording: "Microphone recording",
  manage_workspaces: "Manage workspaces",
  spawn_agents: "Spawn agents",
  agent_commands: "Agent commands",
  surfaces: "Live surfaces",
  write_tabs: "Write to tabs",
  send_input: "Send tab input",
  read_tabs: "Read tabs",
  read_output: "Read tab output",
  read_audio: "Voice notes",
  network: "Network access",
  subprocess: "Local tools",
  secrets: "Secret store",
  analytics: "Usage analytics",
  manage_crons: "Scheduled jobs",
  admin: "Full control",
  chat_backend: "Chat tab",
  chat_preprocessor: "Message transforms",
};

export function permissionChipLabel(id: string): string {
  const known = PERMISSION_CHIP_LABELS[id];
  if (known) return known;
  const words = id.replace(/_/g, " ").trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : id;
}

/** Human-readable line for a capability id (Chrome-style permission wording). */
export const CAPABILITY_LABELS: Record<string, string> = {
  local_recording: "Start and stop your selected microphone recording",
  manage_workspaces: "Create & manage its workspaces",
  spawn_agents: "Spawn AI agents",
  agent_commands: "Receive commands from your agents",
  surfaces: "Drive its live surfaces for your agents",
  write_tabs: "Type into its tabs",
  send_input: "Type into its tabs",
  read_tabs: "Read its tabs",
  read_output: "Read its tabs' output",
  read_audio: "Read your voice notes",
  network: "Network & mesh access",
  subprocess: "Run allowed local tools",
  secrets: "Use its own secret store",
  analytics: "Read this machine's usage analytics",
  manage_crons: "Schedule recurring tasks",
  admin: "Full control",
  chat_backend: "Drive a chat tab",
  chat_preprocessor: "Augment your messages",
};

export function capabilityLabel(id: string): string {
  return CAPABILITY_LABELS[id] ?? id;
}
