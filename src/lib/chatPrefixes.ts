//! Recognises codeterm's own message-injection prefixes and claude's
//! slash-command XML wrappers inside user chat content. Lets the UI
//! render them as typed bubbles (cross-pane / telegram / mesh / slash-
//! command) instead of raw text.

export type { ChatOrigin, SlashCommand } from "@codeterm/chat-core";
import type { ChatOrigin, SlashCommand } from "@codeterm/chat-core";

export interface PrefixMatch {
  origin: ChatOrigin;
  /** The user's actual message body with the wrapper stripped. May be
   *  empty for command-only entries. */
  body: string;
}

// Cross-pane marker format: `[from <Title> pane=<6+hex>] body`.
const PANE_RE = /^\[from\s+(.+?)\s+pane=([0-9a-f]{6,})\]\s*/;

// Bridge services emit `[<Capitalized>: <user_id>[ msg_id=<id>]] body`.
const BRIDGE_RE = /^\[([A-Z][a-zA-Z]+):\s+([^\s\]]+?)(?:\s+msg_id=([^\]]+))?\]\s*/;

// Claude command wrappers — capture each tag individually.
const CMD_TAG_RE =
  /<(local-command-caveat|command-name|command-message|command-args|local-command-stdout)>([\s\S]*?)<\/\1>/g;

// Matches a bare /command line (new claude JSONL format).
const INLINE_CMD_RE = /^\/([\w-]+)\s*(.*)$/;

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
  // Collect all known tag blocks in document order, preserving position.
  type Tag = { kind: string; value: string; start: number };
  const tags: Tag[] = [];
  CMD_TAG_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = CMD_TAG_RE.exec(content)) !== null) {
    tags.push({ kind: m[1], value: m[2], start: m.index });
  }

  // If no tags AND no inline /cmd line → plain user message.
  const hasInlineCmd = /^\s*\/[a-zA-Z][\w-]*(?:\s|$)/m.test(content);
  if (tags.length === 0 && !hasInlineCmd) return null;

  // Strip all known tags + the noise "No response requested." marker.
  CMD_TAG_RE.lastIndex = 0;
  const stripped = content
    .replace(CMD_TAG_RE, "")
    .replace(/^\s*No response requested\.\s*$/gm, "")
    .trim();

  // Separate remaining lines into /cmd lines vs body text.
  const lines = stripped.split("\n").map((l) => l.trim()).filter(Boolean);
  const inlineCommands: Array<{ name: string; args?: string }> = [];
  const bodyParts: string[] = [];
  for (const line of lines) {
    const mc = INLINE_CMD_RE.exec(line);
    if (mc) {
      inlineCommands.push({
        name: `/${mc[1]}`,
        args: mc[2].trim() || undefined,
      });
    } else {
      bodyParts.push(line);
    }
  }

  // Extract structured tags from original scan.
  const stdouts = tags
    .filter((t) => t.kind === "local-command-stdout")
    .map((t) => t.value.trim())
    .filter(Boolean);
  const cmdNameTags = tags
    .filter((t) => t.kind === "command-name")
    .map((t) => t.value.trim());
  const argTags = tags
    .filter((t) => t.kind === "command-args")
    .map((t) => t.value.trim());

  let commands: SlashCommand[];
  if (inlineCommands.length > 0) {
    // New format: /cmd lines outside any wrapper, paired with stdouts by order.
    commands = inlineCommands.map((c, i) => ({
      ...c,
      stdout: stdouts[i] || undefined,
    }));
    // Trailing extra stdouts that had no /cmd line → surface with Claude's real wrapper tag, so the UI doesn't invent a generic "(output)" label.
    for (let i = inlineCommands.length; i < stdouts.length; i++) {
      commands.push({ name: "local-command-stdout", stdout: stdouts[i] });
    }
  } else if (cmdNameTags.length > 0) {
    // Old format: tag-wrapped commands.
    commands = cmdNameTags.map((name, i) => ({
      name: name.startsWith("/") ? name : `/${name}`,
      args: argTags[i] || undefined,
      stdout: stdouts[i] || undefined,
    }));
  } else if (stdouts.length > 0) {
    // Orphan stdouts with no command association.
    commands = stdouts.map((s) => ({ name: "local-command-stdout", stdout: s }));
  } else {
    commands = [];
  }

  if (commands.length === 0 && bodyParts.length === 0) return null;

  return {
    origin: { kind: "command", commands },
    body: bodyParts.join("\n"),
  };
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
