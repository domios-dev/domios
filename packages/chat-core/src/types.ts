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
  | "shell_command"
  | "context_request"
  | "watcher_verdict"
  | "tool_flow"
  | "custom";

export type AgentActivityState = "started" | "running" | "completed" | "failed" | "interrupted" | "closed" | "unknown";

/** One action a sub-agent recorded in its own transcript. */
export interface AgentActivityAction {
  id?: string;
  kind: ToolEventKind;
  summary: string;
  detail?: string;
  description?: string;
  status: "pending" | "success" | "error";
}

/** Sub-agent activity proven by the row's own session record; absent fields are unknown, never inferred. */
export interface AgentActivity {
  agentId?: string;
  agentPath?: string;
  state?: AgentActivityState;
  /** The call that launched the agent; every record of one launch shares it. */
  callId?: string;
  description?: string;
  model?: string;
  durationMs?: number;
  toolUses?: number;
  tokens?: number;
  result?: string;
  actions?: AgentActivityAction[];
  /** Message direction between two agent references (thread id or path). */
  messageFrom?: string;
  messageTo?: string;
}

/** Lifecycle status of a `shell_command` chat item (OSC 133/633 boundaries). */
export type ShellCommandStatus = "running" | "ok" | "failed";

export type ToolEventKind =
  | "command"
  | "edit"
  | "read"
  | "search"
  | "task"
  | "fetch"
  | "codeterm"
  | "web"
  | "image"
  /** Sub-agent activity; `toolInput` carries agent_id, agent_path, agent_state, agent_call_id, message_from, message_to and optional agent_* facts. */
  | "agent"
  | "generic";

/** Provider-neutral web or image result; every string is the source's exact value, absent when unreported. */
export interface ToolRichResult {
  kind: "web" | "image";
  origin?: "generated" | "viewed";
  query?: string;
  resultCount?: number;
  action?: string;
  url?: string;
  pattern?: string;
  path?: string;
  status?: string;
  prompt?: string;
}

export interface ToolEdit {
  path: string;
  old: string;
  new: string;
}

/** One hunk as the provider supplied it; every line keeps its ` `, `+`, `-` or `\` marker. */
export interface FileDiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
}

/** One changed file; counts are the marked lines of its own hunks. */
export interface FileDiff {
  path: string;
  movePath?: string;
  additions: number;
  deletions: number;
  hunks: FileDiffHunk[];
}

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

export type ChatTimelineMarker =
  | { kind: "queued" }
  | { kind: "external_edit"; path: string }
  | { kind: "context_compaction" };

export interface ChatSessionIntent {
  title?: string;
  goal?: string;
}

/** Discriminated union, open for future interactive session-event kinds. */
export type InteractiveElement = InteractiveQuestion;

export interface NormalizedChatMessage {
  id: string;
  /** The id the source emitted before the transcript re-keyed the row; never a render key. */
  sourceMessageId?: string;
  type: ChatMessageKind;
  content: string;
  attachments?: ChatAttachment[];
  timestamp?: string;
  /** Monotonic source-order ordinal supplied by the server (JSONL line/rich-message
   *  index, structured row position). Stable tiebreaker when timestamps are coarse
   *  (codex floors to seconds) or absent (cursor emits none) — see `mergeById`. */
  seq?: number;
  /** Frontend join context for a parser sequence; sequence values repeat when a
   * tab rotates to a new agent session. */
  sourceSessionId?: string;
  /** Frontend-only arrival/send order hint for live optimistic merges. Providers
   *  do not emit this; refresh paths rely on `timestamp`/`seq` instead. */
  clientOrder?: number;
  /** Stable client send-correlation token. Set on the optimistic row and echoed
   *  by the server onto the committed user turn, so an optimistic bubble
   *  reconciles with its echo by identity — never content, which diverges once
   *  the echo is enriched with entity-reference chips. */
  correlationId?: string;
  /** Persisted provider-context generation reconstructed from reset markers. */
  contextEpoch?: number;
  provider?: string;
  subtype?: string;
  tagName?: string;
  systemSubtype?: string;
  timelineMarker?: ChatTimelineMarker;
  sessionIntent?: ChatSessionIntent;
  sourceType?: string;
  sourceSubtype?: string;
  level?: string;
  isCompactSummary?: boolean;
  isVisibleInTranscriptOnly?: boolean;
  imageData?: string;
  imageMimeType?: string;
  toolName?: string;
  toolArgs?: string;
  toolInput?: Record<string, unknown>;
  toolId?: string;
  toolResult?: string;
  toolError?: boolean;
  toolKind?: ToolEventKind;
  toolSummary?: string;
  toolDetail?: string;
  toolEdits?: ToolEdit[];
  /** Human description supplied by the call's source; absent means unknown, never inferred. */
  toolDescription?: string;
  /** Short action title derived only from the call's own data. */
  toolTitle?: string;
  /** Flow identity the source proves. A `tool_flow` row (content `start`/`end`) opens or closes a flow whose
   *  source emits no descriptions, so its calls stream by title; call-stamped flows stream only when described. */
  toolFlowId?: string;
  /** True when this action re-reports a command that another call in its flow already represents. */
  toolEcho?: boolean;
  /** Exact source row identity for a call/result link, when the source proves one. */
  toolCallSourceId?: string;
  /** Result-link evidence state; unavailable evidence must not use positional pairing. */
  toolPairing?: "exact" | "unavailable";
  /** Source-provided command outcome facts. Missing facts stay absent. */
  toolExitCode?: number;
  toolDurationMs?: number;
  toolInterrupted?: boolean;
  toolBackgroundTaskId?: string;
  /** Per-file diffs from the provider's own edit record; absent when none was supplied or it was malformed. */
  fileDiffs?: FileDiff[];
  toolRichResult?: ToolRichResult;
  collapsed?: boolean;
  duration?: string;
  payload?: unknown;
  fromLabel?: string;
  fromTab?: string;
  fromModel?: string;
  fromMesh?: string;
  inboundKind?: "tab" | "report" | "reminder" | "system" | "mesh" | "telegram";
  /** Interactive prompts the provider extracted from this message (answerable widgets). */
  interactive?: InteractiveElement[];
  /** `shell_command` items only — the command line, lifecycle status, exit code
   *  and cwd surfaced from the shell-integration accumulators (OSC 133/633/7). */
  shellCommand?: string;
  shellStatus?: ShellCommandStatus;
  shellExitCode?: number;
  shellCwd?: string;
  /** False in A/D-only degraded mode (no pre-exec `C`); decorations stay valid. */
  shellHasOutputStart?: boolean;
}

export interface ProviderChatAdapter<TMessage extends NormalizedChatMessage = NormalizedChatMessage> {
  parseSessionJsonl(raw: string): TMessage[];
  parseTerminalOutput?: (raw: string) => TMessage[];
}

export type ChatOrigin =
  | {
      kind: "tab";
      title: string;
      tabIdShort: string;
      /** Sender's model family (opus/sonnet/haiku/codex/…) for the avatar.
       *  The recipient is always the current tab — never embedded. */
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
  | { kind: "system"; context?: string }
  | { kind: "system_prompt" }
  | {
      kind: "reminder";
      /** Display name of whoever queued the reminder, when resolvable. */
      whoSet?: string;
      /** Creator tab short id. */
      tab?: string;
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
