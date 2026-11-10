import {CLAUDE_SLASH_COMMANDS, type SlashCommandSuggestion} from "./slashCommands";
export type InitializeCapabilities = import("../../gen/InitializeCapabilities").InitializeCapabilities;
type WireProbeSnapshot = import("../../gen/ProbeSnapshot").ProbeSnapshot;
export type ProbeSnapshot = Exclude<WireProbeSnapshot, { status: "unavailable" }> | (Omit<Extract<WireProbeSnapshot, { status: "unavailable" }>, "reason"> & { reason: string });
export function cachedSlashCommands(snapshot:ProbeSnapshot | null | undefined):SlashCommandSuggestion[] {
  if (!snapshot) return CLAUDE_SLASH_COMMANDS;
  if (snapshot.status!=="ready") return [];
  return (snapshot.data.commands ?? []).filter(command=>command.name.length>0).map(command=>({
    name: command.name.startsWith("/") ? command.name : `/${command.name}`,
    description: command.description ?? "", argumentHint: command.argument_hint ?? undefined,
  }));
}
export function slashCommandsEqual(a: readonly SlashCommandSuggestion[], b: readonly SlashCommandSuggestion[]): boolean {
  return a === b || (a.length === b.length && a.every((command, index) => {
    const other = b[index];
    return command.name === other.name && command.description === other.description && command.argumentHint === other.argumentHint;
  }));
}
export function capabilityBadge(snapshot:ProbeSnapshot | null | undefined):string | null {
  if (!snapshot) return null;
  if (snapshot.status==="pending") return "Harness capabilities pending";
  if (snapshot.status==="unavailable") return `Provider capabilities unavailable (${snapshot.reason})`;
  const data=snapshot.data;
  return [data.account?.subscription_type,data.account?.api_provider,
    data.current_permission_mode ? `Mode: ${data.current_permission_mode}` : null,
    data.fast_mode_state ? `Fast mode: ${data.fast_mode_state}` : null].filter(Boolean).join(" · ") || "Harness capabilities available";
}
