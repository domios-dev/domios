//! Recognises codeterm's own message-injection prefixes and claude's
//! slash-command XML wrappers inside user chat content. Lets the UI
//! render them as typed bubbles (cross-pane / telegram / mesh / slash-
//! command) instead of raw text.

export type ChatOrigin =
  | { kind: "pane"; title: string; paneIdShort: string }
  | { kind: "telegram"; userId: string; messageId?: string }
  | { kind: "mesh"; peerId: string }
  | { kind: "service"; service: string; userId: string }
  | { kind: "command"; commands: SlashCommand[] };

export interface SlashCommand {
  /** The command name including leading slash, e.g. "/model". */
  name: string;
  /** Optional inline arg string the user typed after the command. */
  args?: string;
  /** Optional captured stdout from local execution. */
  stdout?: string;
}

export interface PrefixMatch {
  origin: ChatOrigin;
  /** The user's actual message body with the wrapper stripped. May be
   *  empty for command-only entries. */
  body: string;
}

// Cross-pane: `[from <Title with spaces> pane=89186f68] body`.
// Title may contain spaces / unicode. pane= is followed by 6+ hex chars.
const PANE_RE = /^\[from\s+(.+?)\s+pane=([0-9a-f]{6,})\]\s*/;

// Bridge services emit `[<Capitalized>: <user_id>[ msg_id=<id>]] body`.
const BRIDGE_RE = /^\[([A-Z][a-zA-Z]+):\s+([^\s\]]+?)(?:\s+msg_id=([^\]]+))?\]\s*/;

// Claude command wrappers — capture each tag individually.
const CMD_TAG_RE =
  /<(local-command-caveat|command-name|command-message|command-args|local-command-stdout)>([\s\S]*?)<\/\1>/g;

/**
 * Inspect a user-message content string and return prefix metadata if
 * recognised. Returns null when the content is a plain user message.
 *
 * Priority order: codeterm injection prefix > claude command wrappers.
 */
export function detectChatPrefix(content: string): PrefixMatch | null {
  // Codeterm cross-pane prefix
  const paneMatch = PANE_RE.exec(content);
  if (paneMatch) {
    return {
      origin: { kind: "pane", title: paneMatch[1].trim(), paneIdShort: paneMatch[2] },
      body: content.slice(paneMatch[0].length),
    };
  }
  // Bridge prefix (Telegram, Mesh, Slack, …)
  const bridgeMatch = BRIDGE_RE.exec(content);
  if (bridgeMatch) {
    const service = bridgeMatch[1];
    const userId = bridgeMatch[2];
    const msgId = bridgeMatch[3];
    const body = content.slice(bridgeMatch[0].length);
    if (service === "Telegram") {
      return { origin: { kind: "telegram", userId, messageId: msgId }, body };
    }
    if (service === "Mesh") {
      return { origin: { kind: "mesh", peerId: userId }, body };
    }
    return { origin: { kind: "service", service, userId }, body };
  }
  // Claude slash-command wrappers — possibly multiple per content string.
  return collectSlashCommands(content);
}

function collectSlashCommands(content: string): PrefixMatch | null {
  const tags: { kind: string; value: string }[] = [];
  CMD_TAG_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = CMD_TAG_RE.exec(content)) !== null) {
    tags.push({ kind: m[1], value: m[2] });
  }
  if (tags.length === 0) return null;

  // Group tags into commands. Each command starts at a `command-name` tag;
  // subsequent tags up to the next `command-name` (or end) belong to it.
  // `local-command-caveat` and `command-message` are noise — dropped.
  const commands: SlashCommand[] = [];
  let current: Partial<SlashCommand> | null = null;
  for (const t of tags) {
    if (t.kind === "command-name") {
      if (current?.name) commands.push(current as SlashCommand);
      current = { name: t.value.trim() };
    } else if (current) {
      if (t.kind === "command-args") {
        const v = t.value.trim();
        if (v) current.args = v;
      } else if (t.kind === "local-command-stdout") {
        const v = t.value.trim();
        if (v) current.stdout = v;
      }
      // command-message and local-command-caveat: intentionally dropped.
    }
  }
  if (current?.name) commands.push(current as SlashCommand);
  if (commands.length === 0) return null;

  // Body = whatever survives after stripping all known tags.
  CMD_TAG_RE.lastIndex = 0;
  const body = content.replace(CMD_TAG_RE, "").trim();
  return { origin: { kind: "command", commands }, body };
}

/**
 * Build a display label for an origin (e.g. "Torvalds", "Telegram 123…").
 * Keep short so the chat header doesn't sprawl.
 */
export function labelFor(origin: ChatOrigin): string {
  switch (origin.kind) {
    case "pane":
      return origin.title || "pane";
    case "telegram":
      return `Telegram · ${origin.userId.slice(0, 12)}`;
    case "mesh":
      return `Mesh · ${origin.peerId.slice(0, 10)}`;
    case "service":
      return `${origin.service} · ${origin.userId.slice(0, 12)}`;
    case "command":
      return origin.commands.length === 1
        ? origin.commands[0].name
        : `${origin.commands.length} commands`;
  }
}
