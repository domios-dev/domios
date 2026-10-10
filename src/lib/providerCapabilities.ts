import {CLAUDE_SLASH_COMMANDS, type SlashCommandSuggestion} from "./slashCommands";
export interface InitializeCapabilities {
  commands: Array<{name: string; description: string | null; argument_hint: string | null}> | null;
  models: Array<{id: string; display_name: string | null; description: string | null}> | null;
  agents: Array<{name: string; description: string | null; argument_hint: string | null}> | null;
  account: {subscription_type: string | null; api_provider: string | null} | null;
  current_permission_mode: string | null;
  fast_mode_state: string | null;
  output_style: string | null;
}
export type ProbeSnapshot = {status:"pending"} | {status:"ready";data:InitializeCapabilities} | {status:"unavailable";reason:string};
export function cachedSlashCommands(snapshot:ProbeSnapshot | null | undefined):SlashCommandSuggestion[] {
  if (!snapshot) return CLAUDE_SLASH_COMMANDS;
  if (snapshot.status!=="ready") return [];
  return (snapshot.data.commands ?? []).filter(command=>command.name.length>0).map(command=>({
    name: command.name.startsWith("/") ? command.name : `/${command.name}`,
    description: command.description ?? "", argumentHint: command.argument_hint ?? undefined,
  }));
}
export function capabilityBadge(snapshot:ProbeSnapshot | null | undefined):string | null {
  if (!snapshot) return null;
  if (snapshot.status==="pending") return "Provider capabilities pending";
  if (snapshot.status==="unavailable") return `Provider capabilities unavailable (${snapshot.reason})`;
  const data=snapshot.data;
  return [data.account?.subscription_type,data.account?.api_provider,
    data.current_permission_mode ? `Mode: ${data.current_permission_mode}` : null,
    data.fast_mode_state ? `Fast mode: ${data.fast_mode_state}` : null].filter(Boolean).join(" · ") || "Provider capabilities available";
}
