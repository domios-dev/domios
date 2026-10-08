import { attachmentMarkerSegments } from "./attachmentMarker";
import { fileDiffText } from "./fileDiffs";
import { toolCommandLabel } from "./shellComposition";
import type { ToolEvent } from "./toolEvents";
import type { NormalizedChatMessage } from "./types";

export type ClipRole = "user" | "assistant" | "tool" | "other";

/** One chat row as copy sees it: `source` is exactly what the message holds, attachment tags included. */
export interface ClipMessage {
  id: string;
  role: ClipRole;
  author: string;
  time?: string;
  source: string;
}

/** `plain` is what a reader sees; `portable` keeps attachment tags so a composer paste rebuilds capsules. */
export interface ClipboardFlavours {
  plain: string;
  portable: string;
}

function baseName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Every attachment tag replaced by the capsule label a bubble shows for it. */
export function attachmentTagsAsLabels(source: string): string {
  if (!source.includes("<domios")) return source;
  return attachmentMarkerSegments(source)
    .map((segment) => ("text" in segment ? segment.text : segment.attachment.label ?? baseName(segment.attachment.path)))
    .join("");
}

const ESCAPABLE = /\\([\\`*_{}[\]()#+\-.!>~|])/g;

function inlinePlain(text: string): string {
  return text
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g, (_, label: string, url: string) => (label === url ? url : `${label} (${url})`))
    .replace(/<((?:https?|mailto):[^>\s]+)>/g, "$1")
    .replace(/(\*\*|__)(?=\S)([\s\S]*?\S)\1/g, "$2")
    .replace(/(^|[^\w*])\*(?=\S)([^*\n]*?\S)\*(?![\w*])/g, "$1$2")
    .replace(/(^|[^\w])_(?=\S)([^_\n]*?\S)_(?!\w)/g, "$1$2")
    .replace(/~~(?=\S)([\s\S]*?\S)~~/g, "$1")
    .replace(ESCAPABLE, "$1");
}

function inlineWithCode(line: string): string {
  let out = "";
  let cursor = 0;
  for (const match of line.matchAll(/(`+)([\s\S]*?[^`])\1(?!`)/g)) {
    const start = match.index ?? 0;
    out += inlinePlain(line.slice(cursor, start));
    out += match[2].replace(/^ (.*) $/, "$1");
    cursor = start + match[0].length;
  }
  return out + inlinePlain(line.slice(cursor));
}

function blockPlain(line: string): string {
  if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) return "";
  return line
    .replace(/^\s{0,3}(?:>\s?)+/, "")
    .replace(/^\s{0,3}#{1,6}\s+(.*?)(?:\s+#+\s*)?$/, "$1")
    .replace(/^(\s*)[-*+]\s+\[( |x|X)\]\s+/, (_, indent: string, mark: string) => `${indent}${mark === " " ? "☐" : "☑"} `)
    .replace(/^(\s*)[-*+]\s+/, "$1• ");
}

/** The text a rendered markdown message shows: markup removed, code kept verbatim, link targets kept beside their text. */
export function markdownPlainText(markdown: string): string {
  const out: string[] = [];
  let fence: string | null = null;
  for (const line of markdown.split("\n")) {
    const marker = /^\s{0,3}(`{3,}|~{3,})/.exec(line)?.[1];
    if (fence) {
      if (marker && marker[0] === fence[0] && marker.length >= fence.length && line.trim() === marker) fence = null;
      else out.push(line);
      continue;
    }
    if (marker) {
      fence = marker;
      continue;
    }
    out.push(inlineWithCode(blockPlain(line)));
  }
  return out.join("\n");
}

/** What a reader sees of one message. */
export function messagePlainText(message: ClipMessage): string {
  const labelled = attachmentTagsAsLabels(message.source);
  return message.role === "assistant" ? markdownPlainText(labelled) : labelled;
}

export function messageClipboard(message: ClipMessage): ClipboardFlavours {
  return { plain: messagePlainText(message), portable: message.source };
}

/** Copy as Markdown: the exact source in both flavours. */
export function messageMarkdownClipboard(message: ClipMessage): ClipboardFlavours {
  return { plain: message.source, portable: message.source };
}

/** True when the rendered text differs from the source, so "Copy as Markdown" means something. */
export function sourceDiffersFromVisible(message: ClipMessage): boolean {
  return messagePlainText(message) !== message.source;
}

export function messageHeading(message: ClipMessage): string {
  const who = message.role === "tool" ? `Tool · ${message.author}` : message.author;
  return message.time ? `${who} · ${message.time}` : who;
}

/** Several messages in order, each under its role heading; one message copies as itself. */
export function messagesClipboard(messages: readonly ClipMessage[]): ClipboardFlavours {
  if (messages.length === 1) return messageClipboard(messages[0]);
  return {
    plain: messages.map((message) => `${messageHeading(message)}\n${messagePlainText(message)}`).join("\n\n"),
    portable: messagesMarkdown(messages),
  };
}

/** The transcript of the messages as markdown, what an Ask capsule's file holds. */
export function messagesMarkdown(messages: readonly ClipMessage[]): string {
  return messages.map((message) => `### ${messageHeading(message)}\n\n${message.source.replace(/\s+$/, "")}`).join("\n\n");
}

/** A quoted block per message, attributed to its author, what a Quote capsule's file holds. */
export function messagesQuote(messages: readonly ClipMessage[]): string {
  return messages.map((message) => {
    const body = message.source.replace(/\s+$/, "").split(/\r?\n/).map((line) => (line.trim() ? `> ${line}` : ">")).join("\n");
    return `${messageHeading(message)}:\n${body}`;
  }).join("\n\n");
}

/** "Fermi 14:05" for one message, "3 messages" for several. */
export function messagesReference(messages: readonly ClipMessage[]): string {
  if (messages.length !== 1) return `${messages.length} messages`;
  const [message] = messages;
  return message.time ? `${message.author} ${message.time}` : message.author;
}

/** The heading of a reply block: "Codex · 14:05" for one message, "3 messages" for several. */
export function messagesReplyLabel(messages: readonly ClipMessage[]): string {
  if (messages.length !== 1) return `${messages.length} messages`;
  const [message] = messages;
  return message.time ? `${message.author} · ${message.time}` : message.author;
}

const EXCERPT_MAX = 160;

/** The first quoted message as one line of what a reader sees. */
export function messagesExcerpt(messages: readonly ClipMessage[]): string {
  const line = messages.length ? messagePlainText(messages[0]).replace(/\s+/g, " ").trim() : "";
  return line.length > EXCERPT_MAX ? `${line.slice(0, EXCERPT_MAX - 1).trimEnd()}…` : line;
}

function argumentsText(input: Record<string, unknown> | undefined): string {
  if (!input) return "";
  const entries = Object.entries(input).filter(([, value]) => value !== undefined && value !== null && value !== "");
  return entries.length ? JSON.stringify(Object.fromEntries(entries), null, 2) : "";
}

/** A tool row as text: the command or the call's arguments, then its output; card decorations stay out. */
export function toolEventText(event: ToolEvent): string {
  if (event.commandBatch?.length) return event.commandBatch.map(toolEventText).join("\n\n");
  const parts: string[] = [];
  if (event.kind === "command" || event.kind === "codeterm") {
    parts.push(`$ ${toolCommandLabel(event)}`);
  } else {
    const name = event.message.toolName?.trim() || event.kind;
    parts.push(!event.summary || event.summary.startsWith(name) ? event.summary || name : `${name}: ${event.summary}`);
    if (!event.fileDiffs?.length) {
      const args = argumentsText(event.message.toolInput);
      if (args) parts.push(args);
    }
  }
  if (event.fileDiffs?.length) parts.push(event.fileDiffs.map(fileDiffText).join("\n"));
  const output = event.result?.replace(/\s+$/, "");
  if (output) parts.push(output);
  return parts.join("\n");
}

function shellCommandText(message: NormalizedChatMessage): string {
  const output = message.content.replace(/\s+$/, "");
  return output ? `$ ${message.shellCommand}\n${output}` : `$ ${message.shellCommand}`;
}

export interface ClipNames {
  /** Who speaks for the tab's agent, e.g. the tab title. */
  agent: string;
  formatTime: (timestamp: NormalizedChatMessage["timestamp"]) => string | undefined;
}

/** The copyable form of a rendered chat row; a tool row prefers its merged call-and-result event. */
export function clipMessage(message: NormalizedChatMessage, event: ToolEvent | undefined, names: ClipNames): ClipMessage {
  const time = names.formatTime(message.timestamp);
  const base = { id: message.id, ...(time ? { time } : {}) };
  if (event) return { ...base, role: "tool", author: event.message.toolName?.trim() || event.kind, source: toolEventText(event) };
  if (message.type === "shell_command" && message.shellCommand) return { ...base, role: "tool", author: "Shell", source: shellCommandText(message) };
  if (message.type === "user") return { ...base, role: "user", author: message.fromLabel?.trim() || (message.inboundKind ? "Inbound" : "You"), source: message.content };
  if (message.type === "assistant") return { ...base, role: "assistant", author: names.agent, source: message.content };
  return { ...base, role: "other", author: message.type.replace(/_/g, " "), source: message.content };
}
