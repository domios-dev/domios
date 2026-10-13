//! Recognises codeterm's own message-injection prefixes and claude's
//! slash-command XML wrappers inside user chat content. Lets the UI
//! render them as typed bubbles (cross-tab / Telegram / plugin / Pebble / mesh
//! / slash-command) instead of raw text.

export type { ChatOrigin, SlashCommand } from "@codeterm/chat-core";
import type { ChatOrigin, SlashCommand } from "@codeterm/chat-core";
import { automationOrigin } from "@codeterm/chat-core";
import {
  buildDomiosMessage,
  DOMIOS_CLOSE_TAG,
  parseDomiosMessage,
  parseMessageParts,
  parseMarker,
  systemOriginLabel,
} from "@codeterm/chat-core";

export interface PrefixMatch {
  origin: ChatOrigin;
  /** The user's actual message body with the wrapper stripped. May be
   *  empty for command-only entries. */
  body: string;
}

// Claude command wrappers — capture each tag individually.
const CMD_TAG_RE =
  /<(local-command-caveat|command-name|command-message|command-args|local-command-stdout)>([\s\S]*?)<\/\1>/g;

const PASTED_CONTENT_TAG_RE = /<[\\/]{0,2}pasted_content(?:\s+id="[^"]*")?\s*>/g;

/** Claude transcripts wrap pasted text in `<pasted_content id="…">` tags, which hide leading markers. */
export function stripPastedContentTags(content: string): string {
  if (!content.includes("pasted_content")) return content;
  return content
    .replace(/<pasted_content(?:\s+id="[^"]*")?\s*>\r?\n/g, "")
    .replace(/\r?\n<\/pasted_content(?:\s+id="[^"]*")?\s*>/g, "")
    .replace(PASTED_CONTENT_TAG_RE, "")
    .replace(/\s+$/, "");
}

// Matches a bare /command line (new claude JSONL format).
const INLINE_CMD_RE = /^\/([\w-]+)\s*(.*)$/;

/**
 * Inspect a user-message content string and return prefix metadata if
 * recognised. Returns null when the content is a plain user message.
 *
 * Priority order: codeterm injection prefix > claude command wrappers.
 */
export function detectChatPrefix(content: string): PrefixMatch | null {
  const transport = detectTransportPrefix(content);
  if (transport) return transport;
  // Claude slash-command wrappers — possibly multiple per content string.
  return collectSlashCommands(content);
}

function detectTransportPrefix(content: string): PrefixMatch | null {
  const domios = content.startsWith("<domios")
    ? parseTranscriptDomiosMessage(stripPastedContentTags(content))
    : null;
  if (domios) return domiosPrefix(domios.attrs, domios.body);
  const marker = parseMarker(content);
  if (!marker) return null;
  if (marker.kind === "plugin_config" && marker.attrs.plugin) {
    return {
      origin: {
        kind: "plugin_config",
        pluginId: marker.attrs.plugin,
        name: marker.attrs.name ?? marker.attrs.plugin,
      },
      body: marker.payload,
    };
  }
  if (marker.kind === "system_prompt") {
    return {
      origin: { kind: "system_prompt" },
      body: marker.payload,
    };
  }
  return null;
}

function parseTranscriptDomiosMessage(content: string) {
  const parsed = parseDomiosMessage(content);
  if (parsed) return parsed;
  const close = content.indexOf(DOMIOS_CLOSE_TAG);
  if (close < 0) return null;
  const end = close + DOMIOS_CLOSE_TAG.length;
  const prefix = parseDomiosMessage(content.slice(0, end));
  if (!prefix || !REORDERED_FRAME_CONTENT.test(prefix.content)) return null;
  return parseDomiosMessage(buildDomiosMessage(prefix.attrs, `${content.slice(end)}${prefix.content}`));
}

// Some transcripts store a large paste after the typed frame, leaving only the reply hint inside it.
const REORDERED_FRAME_CONTENT = /^\s*(?:\[to reply, run: [^\]\n]*\])?\s*$/;

function domiosPrefix(attrs: Record<string, string>, body: string): PrefixMatch | null {
  const automation = automationOrigin(attrs);
  if (automation) return { origin: automation, body };
  const from = attrs.from;
  if (from === "tab" || from === "report") {
    if (!attrs.tab) return null;
    return {
      origin: {
        kind: "tab",
        title: attrs.label ?? "tab",
        tabIdShort: attrs.tab,
        ...(attrs.model ? { model: attrs.model } : {}),
        ...(attrs.mesh ? { mesh: attrs.mesh } : {}),
        ...(from === "report" ? { report: true } : {}),
      },
      body,
    };
  }
  if (from === "telegram" && attrs.user) {
    return { origin: { kind: "telegram", userId: attrs.user, messageId: attrs.msg }, body };
  }
  if (from === "mesh") {
    const peerId = attrs.peer ?? attrs.mesh;
    if (peerId) return { origin: { kind: "mesh", peerId }, body };
  }
  if (from === "pebble") {
    return attrs.plugin
      ? { origin: { kind: "plugin", pluginId: attrs.plugin, name: attrs.source }, body }
      : { origin: { kind: "pebble" }, body };
  }
  if (from === "plugin" && attrs.plugin) {
    return { origin: { kind: "plugin", pluginId: attrs.plugin }, body };
  }
  if (from === "system") {
    return { origin: { kind: "system", ...(attrs.context ? { context: attrs.context } : {}) }, body };
  }
  if (from === "reminder") {
    return {
      origin: {
        kind: "reminder",
        ...(attrs.label ? { whoSet: attrs.label } : {}),
        ...(attrs.tab ? { tab: attrs.tab } : {}),
        ...(attrs.model ? { model: attrs.model } : {}),
        ...(attrs.set_at ? { setAt: attrs.set_at } : {}),
        ...(attrs.fire_at ? { fireAt: attrs.fire_at } : {}),
        ...(attrs.id ? { id: attrs.id } : {}),
        ...(attrs.every_ms ? { everyMs: attrs.every_ms } : {}),
        ...(attrs.next_at ? { nextAt: attrs.next_at } : {}),
        ...(attrs.skipped ? { skippedTicks: attrs.skipped } : {}),
      },
      body,
    };
  }
  return null;
}

export function splitChatPrefixedMessages(content: string): Array<{ origin?: ChatOrigin; body: string }> | null {
  const parts = parseMessageParts(content);
  if (!parts.some((part) => part.kind === "frame")) return null;
  return parts.map((part) => {
    const prefix = part.kind === "frame" ? detectTransportPrefix(part.content) : null;
    return prefix ?? { body: part.content };
  });
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
    case "tab":
      return origin.title || "tab";
    case "telegram":
      return `Telegram · ${origin.userId.slice(0, 12)}`;
    case "pebble":
      return "Pebble";
    case "plugin":
      return origin.name || origin.pluginId;
    case "mesh":
      return `Mesh · ${origin.peerId.slice(0, 10)}`;
    case "service":
      return `${origin.service} · ${origin.userId.slice(0, 12)}`;
    case "plugin_config":
      return `Configure · ${origin.name}`;
    case "system":
      return systemOriginLabel(origin.context);
    case "automation":
      return `${origin.source === "utility" ? origin.purpose ?? "shell" : origin.source} · ${origin.name}`;
    case "system_prompt":
      return "System prompt";
    case "reminder":
      return origin.whoSet ? `Reminder · ${origin.whoSet}` : "Reminder";
    case "command":
      return origin.commands.length === 1
        ? origin.commands[0].name
        : `${origin.commands.length} commands`;
  }
}
