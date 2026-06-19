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

export interface InteractiveOption {
  label: string;
  description?: string;
}

/** A question prompt (AskUserQuestion) surfaced by the provider for in-chat answering. */
export interface InteractiveQuestion {
  type: "question";
  id: string;
  toolUseId: string;
  prompt: string;
  header?: string;
  options: InteractiveOption[];
  multiSelect: boolean;
}

/** Discriminated union, open for future interactive session-event kinds. */
export type InteractiveElement = InteractiveQuestion;

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
  fromLabel?: string;
  fromPane?: string;
  fromModel?: string;
  fromMesh?: string;
  inboundKind?: "pane" | "report" | "reminder" | "mesh" | "telegram";
  /** Interactive prompts the provider extracted from this message (answerable widgets). */
  interactive?: InteractiveElement[];
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
  | { kind: "plugin_config"; pluginId: string; name: string }
  | { kind: "system_prompt" }
  | {
      kind: "reminder";
      /** Display name of whoever queued the reminder, when resolvable. */
      whoSet?: string;
      /** Creator pane short id. */
      pane?: string;
      model?: string;
      /** Queued-at, ms since epoch (string on the wire). */
      setAt?: string;
      /** Scheduled fire time, ms since epoch (string on the wire). */
      fireAt?: string;
      /** Originating delayed-message id. */
      id?: string;
    }
  | { kind: "command"; commands: SlashCommand[] };

export interface SlashCommand {
  name: string;
  args?: string;
  stdout?: string;
}
