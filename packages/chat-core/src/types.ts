export type ChatMessageKind =
  | "user"
  | "assistant"
  | "tool_call"
  | "tool_result"
  | "thinking"
  | "status"
  | "system"
  | "local_command"
  | "image"
  | "custom";

export interface ChatAttachment {
  id: string;
  mimeType: string;
  data?: string;
  url?: string;
  path?: string;
}

export interface NormalizedChatMessage {
  id: string;
  type: ChatMessageKind;
  content: string;
  attachments?: ChatAttachment[];
  timestamp?: string;
  provider?: string;
  subtype?: string;
  tagName?: string;
  systemSubtype?: string;
  imageData?: string;
  imageMimeType?: string;
  toolName?: string;
  toolArgs?: string;
  toolInput?: Record<string, unknown>;
  toolId?: string;
  toolResult?: string;
  collapsed?: boolean;
  duration?: string;
  payload?: unknown;
}

export interface ProviderChatAdapter<TMessage extends NormalizedChatMessage = NormalizedChatMessage> {
  parseSessionJsonl(raw: string): TMessage[];
  parseTerminalOutput?: (raw: string) => TMessage[];
}

export type ChatOrigin =
  | {
      kind: "pane";
      title: string;
      paneIdShort: string;
      /** Sender's model family (opus/sonnet/haiku/codex/…) for the avatar.
       *  The recipient is always the current pane — never embedded. */
      model?: string;
      /** Peer machine name when the message crossed the mesh. */
      mesh?: string;
      /** True for hierarchy/cron reports (`[report from …]`). */
      report?: boolean;
    }
  | { kind: "telegram"; userId: string; messageId?: string }
  | { kind: "mesh"; peerId: string }
  | { kind: "service"; service: string; userId: string }
  | { kind: "command"; commands: SlashCommand[] };

export interface SlashCommand {
  name: string;
  args?: string;
  stdout?: string;
}
