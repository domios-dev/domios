import React, { useState, useMemo, useCallback, useEffect, useRef } from "react";
import type {
  ChatOrigin,
  NormalizedChatMessage as ChatMessage,
  ChatAttachment as ImageAttachment,
  SlashCommand,
} from "@codeterm/chat-core";
import { parseMarker } from "@codeterm/chat-core";
import {
  ChevronDown, File, Image, Download,
  // Tool icons
  Terminal,        // Bash
  FileText,        // Read
  Pencil,          // Write
  Wrench,          // Edit
  Bot,             // Agent / Task
  Search,          // Grep, ToolSearch
  Folder,          // Glob
  Sparkles,        // Skill
  RefreshCw,       // Update
  // Origin icons
  ArrowRight,      // pane -> pane
  Send,            // generic chat (telegram / service)
  Network,         // mesh
  ChevronsRight,   // slash-command chip
  CornerUpLeft,    // report (bubbled up to parent)
  Info,
  Copy,
  Check,
  Maximize2,
  Minimize2,
  Mic,             // voice note
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import * as prismStyles from "react-syntax-highlighter/dist/esm/styles/prism";
import { recognize } from "./codeterm/recognize";
import { CodetermCard } from "./codeterm/CodetermCard";

const vscDarkPlus =
  ((prismStyles as { vscDarkPlus?: Record<string, React.CSSProperties> }).vscDarkPlus ??
    (prismStyles as { default?: { vscDarkPlus?: Record<string, React.CSSProperties> } }).default?.vscDarkPlus);

const TOOL_ICONS: Record<string, LucideIcon> = {
  Bash: Terminal,
  Read: FileText,
  Write: Pencil,
  Edit: Wrench,
  Agent: Bot,
  Task: Bot,
  Grep: Search,
  Glob: Folder,
  Skill: Sparkles,
  Update: RefreshCw,
  ToolSearch: Search,
};

const PRE_WRAP_CLASS = "whitespace-pre-wrap break-words [overflow-wrap:anywhere]";
const TEXT_WRAP_CLASS = "break-words [overflow-wrap:anywhere]";
const LONG_CODE_PREVIEW_LINES = 300;
const CODE_PREVIEW_HEAD_LINES = 8;
const CODE_PREVIEW_TAIL_LINES = 8;
const CODE_BLOCK_DEFAULT_MAX_HEIGHT = "16rem";
const MERMAID_BLOCK_MAX_HEIGHT = "72rem";
const CODE_BLOCK_PLAIN_FALLBACK_THRESHOLD = 200_000;
const MERMAID_RENDER_LENGTH_LIMIT = 120_000;
const MERMAID_CLEANUP_PASSES = 2;
const MERMAID_FULLSCREEN_MIN_SCALE = 0.35;
const MERMAID_FULLSCREEN_MAX_SCALE = 5;
const MERMAID_CDN_URL = "https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.min.js";
const MERMAID_RENDER_CACHE_LIMIT = 64;
const MERMAID_BLOCK_GUARDS = [
  "flowchart",
  "graph",
  "sequenceDiagram",
  "stateDiagram",
  "stateDiagram-v2",
  "gantt",
  "classDiagram",
  "erDiagram",
  "mindmap",
  "journey",
  "pie",
  "gitGraph",
  "quadrantChart",
  "timeline",
  "packet",
  "kanban",
];
let mermaidLoadPromise: Promise<void> | null = null;
let mermaidInitialized = false;
type CachedMermaidRender = {
  svg: string | null;
  error?: string;
  sourceId?: string;
};
const mermaidRenderCache = new Map<string, CachedMermaidRender>();

let mobileMediaQuery: MediaQueryList | null = null;
let mediaQueryListener: (() => void) | null = null;
let mobileViewport = false;
const mobileViewportSubscribers = new Set<(value: boolean) => void>();
type RectLike = {
  top: number;
  left: number;
  width: number;
  height: number;
};
type PointLike = {
  x: number;
  y: number;
};
type MermaidGesture = {
  distance: number;
  midpoint: PointLike;
  pan: PointLike;
  scale: number;
};

function getMobileViewport(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  if (!mobileMediaQuery) {
    mobileMediaQuery = window.matchMedia("(max-width: 768px)");
    mobileViewport = mobileMediaQuery.matches;
  }
  return mobileViewport;
}

function subscribeMobileViewport(listener: (value: boolean) => void): () => void {
  if (typeof window === "undefined") {
    listener(false);
    return () => {};
  }
  listener(getMobileViewport());
  mobileViewportSubscribers.add(listener);

  if (!mediaQueryListener && mobileMediaQuery) {
    mediaQueryListener = () => {
      mobileViewport = mobileMediaQuery!.matches;
      for (const cb of mobileViewportSubscribers) {
        cb(mobileViewport);
      }
    };
    mobileMediaQuery.addEventListener("change", mediaQueryListener);
  }

  return () => {
    mobileViewportSubscribers.delete(listener);
    if (mobileViewportSubscribers.size === 0 && mediaQueryListener && mobileMediaQuery) {
      mobileMediaQuery.removeEventListener("change", mediaQueryListener);
      mediaQueryListener = null;
      mobileMediaQuery = null;
    }
  };
}

function getCachedMermaidRender(source: string) {
  return mermaidRenderCache.get(source);
}

function setCachedMermaidRender(source: string, value: CachedMermaidRender) {
  if (mermaidRenderCache.has(source)) {
    mermaidRenderCache.delete(source);
  }
  mermaidRenderCache.set(source, value);
  while (mermaidRenderCache.size > MERMAID_RENDER_CACHE_LIMIT) {
    const oldest = mermaidRenderCache.keys().next();
    if (oldest.done) {
      break;
    }
    mermaidRenderCache.delete(oldest.value);
  }
}

function remapMermaidSvgSourceId(svg: string, from: string, to: string) {
  if (!from || !to || from === to) {
    return svg;
  }
  return svg.split(from).join(to);
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function distanceBetween(a: PointLike, b: PointLike) {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function midpointBetween(a: PointLike, b: PointLike): PointLike {
  return {
    x: (a.x + b.x) / 2,
    y: (a.y + b.y) / 2,
  };
}

function removeMermaidRenderArtifacts(sourceId: string) {
  if (typeof document === "undefined") {
    return;
  }
  const tempIdPrefix = `d${sourceId}`;
  const elements = Array.from(document.body.querySelectorAll<HTMLElement>("[id]"));
  for (const element of elements) {
    if (!(element instanceof HTMLElement)) {
      continue;
    }
    const isOwnTempNode = element.id === tempIdPrefix || element.id.startsWith(tempIdPrefix);
    if (isOwnTempNode) {
      element.remove();
    }
  }
}

function ensureMermaidLoaded(): Promise<void> {
  if (typeof window === "undefined") {
    return Promise.resolve();
  }
  if ((window as { mermaid?: unknown }).mermaid) {
    return Promise.resolve();
  }
  if (!mermaidLoadPromise) {
    mermaidLoadPromise = new Promise((resolve, reject) => {
      const existing = document.querySelector<HTMLScriptElement>('script[data-ct-mermaid="1"]');
      if (existing) {
        if (existing.dataset.loaded === "1") {
          resolve();
          return;
        }
        existing.addEventListener("load", () => {
          existing.dataset.loaded = "1";
          resolve();
        }, { once: true });
        existing.addEventListener("error", () => {
          mermaidLoadPromise = null;
          reject(new Error("Failed to load mermaid script"));
        }, { once: true });
        return;
      }

      const script = document.createElement("script");
      script.src = MERMAID_CDN_URL;
      script.async = true;
      script.dataset.ctMermaid = "1";
      script.onload = () => {
        script.dataset.loaded = "1";
        resolve();
      };
      script.onerror = () => {
        mermaidLoadPromise = null;
        reject(new Error("Failed to load mermaid script"));
      };
      document.head.appendChild(script);
    });
  }
  return mermaidLoadPromise;
}

function ensureMermaidInitialized() {
  if (typeof window === "undefined") return false;
  const mermaid = (window as { mermaid?: { initialize?: (...args: unknown[]) => unknown; render?: (...args: unknown[]) => unknown } }).mermaid;
  if (!mermaid || mermaidInitialized) return true;
  mermaid.initialize?.({
    startOnLoad: false,
    securityLevel: "loose",
    theme: "neutral",
  });
  mermaidInitialized = true;
  return true;
}

function extractCodeText(children: unknown): string {
  if (Array.isArray(children)) {
    return children.map((child) => (typeof child === "string" ? child : "")).join("");
  }
  return typeof children === "string" ? children : "";
}

function looksLikeMermaidSource(text: string): boolean {
  const firstLine = text.trim().split("\n").find((line) => line.trim() !== "");
  if (!firstLine || !text.includes("\n")) {
    return false;
  }
  return firstLine.trim().startsWith("---") || MERMAID_BLOCK_GUARDS.some((keyword) => {
    const token = firstLine.trim();
    return token === keyword || token.startsWith(`${keyword} `) || token.startsWith(`${keyword}\t`);
  });
}

function stripMermaidWrapper(text: string): string {
  let value = text.trim();
  for (let i = 0; i < MERMAID_CLEANUP_PASSES; i += 1) {
    const match = value.match(/^\s*```[a-z0-9+#-]*\s*\n([\s\S]*?)\n\s*```\s*$/i);
    if (!match) {
      break;
    }
    value = match[1];
  }
  return value.trim();
}

function stripMermaidLeadingMarker(text: string): string {
  const lines = text.split("\n");
  if (!lines.length) {
    return text;
  }
  const firstLine = lines[0].trim().toLowerCase();
  if (firstLine === "mermaid" || firstLine === "mmd") {
    lines.shift();
  }
  while (lines.length && lines[lines.length - 1].trim() === "```") {
    lines.pop();
  }
  while (lines.length && lines[0].trim() === "```") {
    lines.shift();
  }
  return lines.join("\n").trim();
}

function getMermaidRenderCandidates(text: string): string[] {
  const base = text.replace(/\r/g, "").replace(/^\uFEFF/, "").trim();
  const candidates: string[] = [];
  const seen = new Set<string>();
  const pushCandidate = (value: string) => {
    const candidate = value.trim();
    if (!candidate || seen.has(candidate)) {
      return;
    }
    seen.add(candidate);
    candidates.push(candidate);
  };

  const withoutFences = stripMermaidWrapper(base);
  const fullyTrimmed = stripMermaidLeadingMarker(withoutFences);

  pushCandidate(base);
  pushCandidate(withoutFences);
  pushCandidate(fullyTrimmed);
  pushCandidate(stripMermaidWrapper(fullyTrimmed));
  return candidates;
}

function resolveCodeLanguage(className: string | undefined, children: unknown): string | null {
  const explicit = /language-([a-z0-9+#-]+)/i.exec(className || "");
  if (explicit) {
    return explicit[1].toLowerCase();
  }
  const source = extractCodeText(children).replace(/\r/g, "").trim();
  return looksLikeMermaidSource(source) ? "mermaid" : null;
}

function ToolIcon({ name }: { name: string }) {
  const Icon = TOOL_ICONS[name] ?? Wrench;
  return <Icon size={12} className="inline opacity-70" style={{ verticalAlign: "-2px" }} />;
}

/**
 * Real markdown renderer for assistant text.
 * - GFM tables, task lists, strikethrough via remark-gfm
 * - Syntax-highlighted fenced code blocks via react-syntax-highlighter
 * - All inline elements themed with codeterm CSS vars
 *
 * Note: react-markdown v10 no longer passes `inline` to the code component.
 * Block code is detected by the presence of a `language-*` className (set by
 * remark on fenced blocks). Inline <code> never has that className.
 */
function RichMarkdown({ text }: { text: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        h1: ({ node: _node, ...props }) => (
          <h1 className="text-base font-bold mt-3 mb-1" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        h2: ({ node: _node, ...props }) => (
          <h2 className="text-sm font-semibold mt-2 mb-1" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        h3: ({ node: _node, ...props }) => (
          <h3 className="text-sm font-medium mt-1.5 mb-0.5" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        p: ({ node: _node, ...props }) => <p className={`my-1 ${TEXT_WRAP_CLASS}`} {...props} />,
        ul: ({ node: _node, ...props }) => <ul className="list-disc ml-5 my-1" {...props} />,
        ol: ({ node: _node, ...props }) => <ol className="list-decimal ml-5 my-1" {...props} />,
        li: ({ node: _node, children, ...props }) => (
          <li className="my-0.5" {...props}>{children}</li>
        ),
        table: ({ node: _node, ...props }) => (
          <div className="overflow-x-auto my-2">
            <table
              className="text-xs border-collapse"
              style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)" }}
              {...props}
            />
          </div>
        ),
        thead: ({ node: _node, ...props }) => (
          <thead style={{ background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 8%, transparent)" }} {...props} />
        ),
        th: ({ node: _node, ...props }) => (
          <th
            className="px-2 py-1 text-left font-semibold border"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)" }}
            {...props}
          />
        ),
        td: ({ node: _node, ...props }) => (
          <td
            className="px-2 py-1 border align-top"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 20%, transparent)" }}
            {...props}
          />
        ),
        // Block code: SyntaxHighlighter/CodeBlock. Inline code: styled span.
        // Detection: fenced blocks get a "language-*" className from remark.
        // For language-less blocks, infer Mermaid from known source keywords.
        code: ({ node: _node, className, children, ...props }: React.ComponentProps<"code"> & { node?: unknown }) => {
          const language = resolveCodeLanguage(className, children);
          const plainCode = extractCodeText(children);
          if (!language) {
            return (
              <code
                className="px-1 py-0.5 rounded text-xs font-mono"
                style={{
                  background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
                  color: "var(--ct-fg, #c9d1d9)",
                  overflowWrap: "anywhere",
                  wordBreak: "break-word",
                }}
                {...props}
              >
                {plainCode}
              </code>
            );
          }
          return (
            <CodeBlock
              text={plainCode}
              language={language}
              maxHeight={language.toLowerCase() === "mermaid" ? MERMAID_BLOCK_MAX_HEIGHT : CODE_BLOCK_DEFAULT_MAX_HEIGHT}
              collapsible
              collapseAfterLines={LONG_CODE_PREVIEW_LINES}
            />
          );
        },
        // Strip default <pre> wrapper — SyntaxHighlighter renders its own via PreTag
        pre: ({ node: _node, children }) => <>{children}</>,
        a: ({ node: _node, ...props }) => (
          <a
            target="_blank"
            rel="noopener noreferrer"
            className="underline"
            style={{ color: "var(--ct-accent, #58a6ff)" }}
            {...props}
          />
        ),
        del: ({ node: _node, ...props }) => <del className="opacity-60" {...props} />,
        blockquote: ({ node: _node, ...props }) => (
          <blockquote
            className="border-l-2 pl-3 my-2 italic opacity-80"
            style={{ borderColor: "var(--ct-muted-fg, #8b949e)" }}
            {...props}
          />
        ),
        hr: () => (
          <hr
            className="my-2"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 30%, transparent)" }}
          />
        ),
      }}
    >
      {text}
    </ReactMarkdown>
  );
}

/** Extract file path from tool args or toolInput. */
function extractFilePath(message: ChatMessage): string | null {
  if (message.toolInput) {
    const fp = (message.toolInput.file_path ?? message.toolInput.filePath) as string | undefined;
    if (fp) return fp;
  }
  if (message.toolArgs) {
    try {
      const parsed = JSON.parse(message.toolArgs);
      if (parsed.file_path) return parsed.file_path;
    } catch {
      // Try to extract from text
      const match = message.toolArgs.match(/(?:file_path|path)['":\s]+([^\s'"}\],]+)/);
      if (match) return match[1];
    }
  }
  return null;
}

interface EditPair {
  old: string;
  new: string;
}

/** Extract Edit/MultiEdit before→after pairs from a tool call so the chat can
 *  render a real diff instead of just "file updated". Handles single
 *  old_string/new_string and the MultiEdit `edits` array, from either
 *  `toolInput` or a JSON `toolArgs` fallback. Provider-agnostic. */
function extractEdits(message: ChatMessage): EditPair[] {
  let input: Record<string, unknown> | null =
    (message.toolInput as Record<string, unknown> | undefined) ?? null;
  if (!input && message.toolArgs) {
    try {
      input = JSON.parse(message.toolArgs) as Record<string, unknown>;
    } catch {
      input = null;
    }
  }
  if (!input) return [];
  const toPair = (e: Record<string, unknown>): EditPair | null => {
    // Accept snake_case (claude) and camelCase (opencode et al).
    const oldS = (e.old_string ?? e.oldString) as unknown;
    const newS = (e.new_string ?? e.newString) as unknown;
    if (typeof oldS !== "string" && typeof newS !== "string") return null;
    return { old: String(oldS ?? ""), new: String(newS ?? "") };
  };
  if (Array.isArray(input.edits)) {
    return input.edits
      .map((e) => toPair(e as Record<string, unknown>))
      .filter((e): e is EditPair => e !== null);
  }
  const single = toPair(input);
  return single ? [single] : [];
}

/** Old→new diff for Edit/MultiEdit tool calls: removed lines in red, added in
 *  green. Line-based, no LCS — clear and cheap. */
function DiffView({ edits }: { edits: EditPair[] }) {
  const row = (line: string, kind: "del" | "add", key: string) => (
    <div
      key={key}
      className={`${PRE_WRAP_CLASS} px-1`}
      style={{
        background:
          kind === "del"
            ? "color-mix(in srgb, var(--ct-red, #f85149) 14%, transparent)"
            : "color-mix(in srgb, var(--ct-green, #3fb950) 14%, transparent)",
        color: "var(--ct-fg)",
      }}
    >
      <span style={{ color: kind === "del" ? "var(--ct-red, #f85149)" : "var(--ct-green, #3fb950)" }}>
        {kind === "del" ? "- " : "+ "}
      </span>
      {line || " "}
    </div>
  );
  return (
    <div className="font-mono text-[11px] leading-relaxed overflow-x-auto">
      {edits.map((e, i) => (
        <div key={i} className={i > 0 ? "mt-2 border-t border-[var(--ct-border)] pt-2" : ""}>
          {e.old ? e.old.split("\n").map((l, j) => row(l, "del", `o${i}-${j}`)) : null}
          {e.new ? e.new.split("\n").map((l, j) => row(l, "add", `n${i}-${j}`)) : null}
        </div>
      ))}
    </div>
  );
}

/** Truncate file path to show just filename + parent. */
function shortPath(fullPath: string): string {
  const parts = fullPath.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 2) return fullPath;
  return ".../" + parts.slice(-2).join("/");
}

function isShellToolName(name: string | undefined): boolean {
  const n = (name ?? "").toLowerCase();
  return n === "bash"
    || n === "shell"
    || n === "shell_command"
    || n === "exec"
    || n === "powershell"
    || n.includes("shell")
    || n.includes("bash");
}

/** Extract shell command text across Claude/Codex provider shapes. */
function extractCommand(message: ChatMessage): string | null {
  if (message.toolInput) {
    const cmd = message.toolInput.command as string | undefined;
    if (cmd) return cmd;
  }
  if (message.toolArgs) {
    try {
      const parsed = JSON.parse(message.toolArgs);
      if (parsed && typeof parsed === "object" && typeof parsed.command === "string") {
        return parsed.command;
      }
    } catch {
      if (isShellToolName(message.toolName)) {
        const args = message.toolArgs.trim();
        return args ? args : null;
      }
    }
  }
  return null;
}

function PlainOutput({ text, maxHeight = "12rem" }: { text: string; maxHeight?: string }) {
  return (
    <pre
      className={`${PRE_WRAP_CLASS} m-0 overflow-auto rounded-md border px-2 py-1.5 font-mono text-[11px] leading-relaxed`}
      style={{
        maxHeight,
        color: "var(--ct-fg)",
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 72%, black 10%)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
      }}
    >
      {text}
    </pre>
  );
}

function languageFromPath(path: string | null): string {
  if (!path) return "text";
  const ext = path.split(/[./\\]/).pop()?.toLowerCase();
  const map: Record<string, string> = {
    js: "javascript",
    jsx: "jsx",
    ts: "typescript",
    tsx: "tsx",
    json: "json",
    css: "css",
    scss: "scss",
    html: "html",
    md: "markdown",
    markdown: "markdown",
    mmd: "mermaid",
    mermaid: "mermaid",
    rs: "rust",
    py: "python",
    ps1: "powershell",
    sh: "bash",
    bash: "bash",
    yml: "yaml",
    yaml: "yaml",
    toml: "toml",
    sql: "sql",
    go: "go",
    java: "java",
    c: "c",
    h: "c",
    cpp: "cpp",
    hpp: "cpp",
    cs: "csharp",
  };
  return ext ? (map[ext] ?? ext) : "text";
}

function findScrollableContainer(start: HTMLElement | null): HTMLElement | null {
  if (typeof window === "undefined" || !start) {
    return null;
  }
  let current: HTMLElement | null = start.parentElement;
  while (current) {
    const style = window.getComputedStyle(current);
    const overflowY = style.overflowY;
    const overflowX = style.overflowX;
    const hasYScroll = overflowY === "auto" || overflowY === "scroll" || overflowY === "overlay";
    const hasXScroll = overflowX === "auto" || overflowX === "scroll" || overflowX === "overlay";
    if (hasYScroll || hasXScroll) {
      return current;
    }
    current = current.parentElement;
  }
  return null;
}

/** Last non-empty line of a tool result — used for the compact inline tail. */
function lastNonEmptyLine(text: string): string {
  const lines = text.replace(/\r/g, "").split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const trimmed = lines[i].trim();
    if (trimmed) return trimmed;
  }
  return "";
}

function lineCount(text: string): number {
  if (!text) return 0;
  return text.split("\n").length;
}

type CodeContentProps = {
  fullSize: boolean;
  maxHeight: string;
  displayedText: string;
  language: string;
  mermaidMode: boolean;
  forcePlainText?: boolean;
  mermaidSvg: string | null;
  shouldDisplayMermaid: boolean;
  showMermaidFallback: boolean;
  suppressMermaidErrorOutput: boolean;
  interactiveMermaid?: boolean;
  mermaidPan?: PointLike;
  mermaidScale?: number;
  onMermaidPointerDown?: React.PointerEventHandler<HTMLDivElement>;
  onMermaidPointerMove?: React.PointerEventHandler<HTMLDivElement>;
  onMermaidPointerUp?: React.PointerEventHandler<HTMLDivElement>;
  onMermaidWheel?: React.WheelEventHandler<HTMLDivElement>;
  onMermaidDoubleClick?: React.MouseEventHandler<HTMLDivElement>;
};

const CodeBlockContent = React.memo(function CodeBlockContent({
  fullSize,
  maxHeight,
  displayedText,
  language,
  mermaidMode,
  forcePlainText = false,
  mermaidSvg,
  shouldDisplayMermaid,
  showMermaidFallback,
  suppressMermaidErrorOutput,
  interactiveMermaid = false,
  mermaidPan,
  mermaidScale = 1,
  onMermaidPointerDown,
  onMermaidPointerMove,
  onMermaidPointerUp,
  onMermaidWheel,
  onMermaidDoubleClick,
}: CodeContentProps) {
  if (forcePlainText) {
    return (
      <pre
        className={`${PRE_WRAP_CLASS} m-0 p-3`}
        style={{
          maxHeight: fullSize ? "100%" : maxHeight,
          background: "var(--ct-bg)",
          overflow: "auto",
          color: "var(--ct-fg)",
          whiteSpace: "pre-wrap",
        }}
      >
        {displayedText}
      </pre>
    );
  }
  return (
    <div
      className="overflow-x-auto overflow-y-auto"
      style={{ maxHeight: fullSize ? "100%" : maxHeight }}
    >
      {suppressMermaidErrorOutput ? null : shouldDisplayMermaid ? (
        <div
          className="px-2 py-1.5"
          style={{
            color: "var(--ct-fg)",
            cursor: interactiveMermaid ? "grab" : undefined,
            touchAction: interactiveMermaid ? "none" : undefined,
          }}
          onPointerDown={onMermaidPointerDown}
          onPointerMove={onMermaidPointerMove}
          onPointerUp={onMermaidPointerUp}
          onPointerCancel={onMermaidPointerUp}
          onWheel={onMermaidWheel}
          onDoubleClick={onMermaidDoubleClick}
        >
          <div
            className="w-full overflow-auto flex items-center justify-center"
            style={{
              background: "var(--ct-surface)",
              minHeight: "max(100%, 0px)",
            }}
          >
            <div
              style={{
                transform: interactiveMermaid
                  ? `translate(${mermaidPan?.x ?? 0}px, ${mermaidPan?.y ?? 0}px) scale(${mermaidScale})`
                  : undefined,
                transformOrigin: "center center",
                transition: interactiveMermaid ? "transform 80ms linear" : undefined,
              }}
              dangerouslySetInnerHTML={{ __html: mermaidSvg ?? "" }}
            />
          </div>
        </div>
      ) : (
        <>
          {showMermaidFallback && mermaidMode && (
            <div className="px-2 py-1.5 text-xs opacity-70">Rendering mermaid…</div>
          )}
          <SyntaxHighlighter
            language={language}
            style={vscDarkPlus}
            PreTag="div"
            customStyle={{
              margin: 0,
              borderRadius: 0,
              padding: "0.75rem",
              fontSize: "0.75rem",
              lineHeight: "1.4",
              background: fullSize ? "var(--ct-bg)" : "var(--ct-bg)",
              maxWidth: "100%",
              overflowWrap: "anywhere",
              wordBreak: "break-word",
            }}
            codeTagProps={{
              style: {
                fontFamily: "inherit",
                overflowWrap: "anywhere",
                wordBreak: "break-word",
              },
            }}
            wrapLongLines
          >
            {displayedText}
          </SyntaxHighlighter>
        </>
      )}
    </div>
  );
});

function CodeBlock({
  text,
  language = "text",
  maxHeight = CODE_BLOCK_DEFAULT_MAX_HEIGHT,
  collapsible = false,
  collapseAfterLines = 20,
}: {
  text: string;
  language?: string;
  maxHeight?: string;
  collapsible?: boolean;
  collapseAfterLines?: number;
}) {
  const normalizedText = useMemo(() => text.replace(/\r/g, ""), [text]);
  const codeLines = useMemo(() => normalizedText.replace(/\n$/, "").split("\n"), [normalizedText]);
  const lineCountValue = codeLines.length || 1;
  const mermaidMode = language.toLowerCase() === "mermaid";
  const canCollapse = collapsible && !mermaidMode && lineCountValue > collapseAfterLines;
  const [isMobileViewport, setIsMobileViewport] = useState(() => getMobileViewport());
  const [expanded, setExpanded] = useState(!canCollapse);
  const [copied, setCopied] = useState(false);
  const [mermaidError, setMermaidError] = useState<string | null>(null);
  const [mermaidSvg, setMermaidSvg] = useState<string | null>(null);
  const [mermaidLoading, setMermaidLoading] = useState(false);
  const mermaidCandidates = useMemo(() => (mermaidMode ? getMermaidRenderCandidates(normalizedText) : []), [mermaidMode, normalizedText]);
  const mermaidRenderKey = mermaidCandidates.join("\u0000");
  const shouldUsePlainRenderer = (!mermaidMode && normalizedText.length > CODE_BLOCK_PLAIN_FALLBACK_THRESHOLD)
    || (mermaidMode && (mermaidCandidates[0]?.length ?? 0) > MERMAID_RENDER_LENGTH_LIMIT);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [fullscreenBounds, setFullscreenBounds] = useState<RectLike | null>(null);
  const copyTimer = useRef<number | null>(null);
  const fullscreenBoundsRaf = useRef<number | null>(null);
  const blockRef = useRef<HTMLDivElement>(null);
  const scrollHostRef = useRef<HTMLElement | null>(null);
  const mermaidPointerRef = useRef<Map<number, PointLike>>(new Map());
  const mermaidGestureRef = useRef<MermaidGesture | null>(null);
  const mermaidNodeId = useMemo(() => `ct-mermaid-${Math.random().toString(36).slice(2, 11)}`, []);
  const [mermaidPan, setMermaidPan] = useState<PointLike>({ x: 0, y: 0 });
  const [mermaidScale, setMermaidScale] = useState(1);
  const copyText = useCallback(async (event: React.MouseEvent) => {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(normalizedText);
    } catch {
      const fallback = document.createElement("textarea");
      fallback.value = normalizedText;
      fallback.style.position = "fixed";
      fallback.style.opacity = "0";
      document.body.appendChild(fallback);
      fallback.select();
      document.execCommand("copy");
      document.body.removeChild(fallback);
    }
    setCopied(true);
    if (copyTimer.current) {
      window.clearTimeout(copyTimer.current);
    }
    copyTimer.current = window.setTimeout(() => {
      setCopied(false);
    }, 1400);
  }, [normalizedText]);

  useEffect(() => {
    return subscribeMobileViewport((value) => {
      setIsMobileViewport(value);
    });
  }, []);

  const updateFullscreenBounds = useCallback(() => {
    const host = scrollHostRef.current;
    if (!host) {
      return;
    }
    const rect = host.getBoundingClientRect();
    setFullscreenBounds((previous) => {
      if (
        previous &&
        previous.top === rect.top &&
        previous.left === rect.left &&
        previous.width === rect.width &&
        previous.height === rect.height
      ) {
        return previous;
      }
      return {
        top: rect.top,
        left: rect.left,
        width: rect.width,
        height: rect.height,
      };
    });
  }, []);

  const scheduleBoundsUpdate = useCallback(() => {
    if (fullscreenBoundsRaf.current !== null) {
      return;
    }
    fullscreenBoundsRaf.current = window.requestAnimationFrame(() => {
      fullscreenBoundsRaf.current = null;
      updateFullscreenBounds();
    });
  }, [updateFullscreenBounds]);

  useEffect(() => {
    return () => {
      if (copyTimer.current) {
        window.clearTimeout(copyTimer.current);
      }
      if (fullscreenBoundsRaf.current !== null) {
        window.cancelAnimationFrame(fullscreenBoundsRaf.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!mermaidMode || !expanded || shouldUsePlainRenderer || mermaidCandidates.length === 0) {
      return;
    }

    const primaryCandidate = mermaidCandidates[0];
    const cachedSvgCandidate = mermaidCandidates.find((candidate) => {
      const cached = getCachedMermaidRender(candidate);
      return !!cached?.svg;
    });
    if (cachedSvgCandidate) {
      const cached = getCachedMermaidRender(cachedSvgCandidate);
      const cachedSvg = cached!.sourceId
        ? remapMermaidSvgSourceId(cached!.svg as string, cached!.sourceId, mermaidNodeId)
        : cached!.svg;
      setMermaidLoading(false);
      setMermaidError(null);
      setMermaidSvg(cachedSvg);
      return;
    }

    const allCachedErrors = mermaidCandidates.every((candidate) => {
      const cached = getCachedMermaidRender(candidate);
      return cached !== undefined && cached.svg === null;
    });
    if (allCachedErrors) {
      removeMermaidRenderArtifacts(mermaidNodeId);
      const firstError = mermaidCandidates
        .map((candidate) => getCachedMermaidRender(candidate)?.error)
        .find((value) => value);
      setMermaidLoading(false);
      setMermaidError(firstError ?? "Mermaid parse error");
      setMermaidSvg(null);
      return;
    }

    let cancelled = false;
    setMermaidLoading(true);
    setMermaidError(null);
    setMermaidSvg(null);
    (async () => {
      let lastError: string | null = null;
      try {
        await ensureMermaidLoaded();
        if (cancelled) return;
        ensureMermaidInitialized();
        const mermaid = (window as {
          mermaid?: {
            render?: (...args: unknown[]) => unknown;
          };
        }).mermaid;
        if (!mermaid?.render) {
          throw new Error("Mermaid runtime not available");
        }
        for (const candidate of mermaidCandidates) {
          if (cancelled) return;
          const cached = getCachedMermaidRender(candidate);
          if (cached?.svg === null) {
            continue;
          }
          try {
            removeMermaidRenderArtifacts(mermaidNodeId);
            const rendered = (await Promise.resolve(mermaid.render(mermaidNodeId, candidate) as Promise<{ svg: string } | string>));
            removeMermaidRenderArtifacts(mermaidNodeId);
            if (cancelled) return;
            const svg = typeof rendered === "string" ? rendered : rendered.svg;
            setCachedMermaidRender(candidate, { svg, sourceId: mermaidNodeId });
            setMermaidSvg(svg);
            setMermaidError(null);
            return;
          } catch (candidateError) {
            removeMermaidRenderArtifacts(mermaidNodeId);
            const message = candidateError instanceof Error ? candidateError.message : "Mermaid parse error";
            lastError = message;
            setCachedMermaidRender(candidate, { svg: null, error: message });
          }
        }
      } catch (error) {
        lastError = error instanceof Error ? error.message : "Mermaid parse error";
      } finally {
        removeMermaidRenderArtifacts(mermaidNodeId);
        if (!cancelled) {
          setMermaidLoading(false);
          if (lastError) {
            const message = lastError;
            setMermaidError(message);
            if (primaryCandidate) {
              setCachedMermaidRender(primaryCandidate, { svg: null, error: message });
            }
          }
        }
      }
    })();

    return () => {
      cancelled = true;
      removeMermaidRenderArtifacts(mermaidNodeId);
    };
  }, [mermaidMode, expanded, shouldUsePlainRenderer, mermaidRenderKey, mermaidNodeId, mermaidCandidates.length]);

  useEffect(() => {
    if (typeof window === "undefined" || !isFullscreen) {
      return;
    }
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [isFullscreen]);

  useEffect(() => {
    if (!isFullscreen) {
      return;
    }
    const host = scrollHostRef.current;
    if (!host) {
      return;
    }
    const onScrollOrResize = () => {
      scheduleBoundsUpdate();
    };
    host.addEventListener("scroll", onScrollOrResize, { passive: true });
    window.addEventListener("resize", onScrollOrResize);
    updateFullscreenBounds();
    return () => {
      host.removeEventListener("scroll", onScrollOrResize);
      window.removeEventListener("resize", onScrollOrResize);
    };
  }, [isFullscreen, scheduleBoundsUpdate, updateFullscreenBounds]);

  useEffect(() => {
    if (!canCollapse) {
      setExpanded(true);
      return;
    }
    setExpanded(!isMobileViewport);
  }, [canCollapse, isMobileViewport]);

  const isCollapsed = canCollapse && !expanded;
  const displayedText = useMemo(() => {
    if (!isCollapsed || lineCountValue <= CODE_PREVIEW_HEAD_LINES + CODE_PREVIEW_TAIL_LINES + 1) {
      return normalizedText;
    }
    const head = codeLines.slice(0, CODE_PREVIEW_HEAD_LINES).join("\n");
    const tail = codeLines.slice(-CODE_PREVIEW_TAIL_LINES).join("\n");
    return `${head}\n...\n${tail}`;
  }, [isCollapsed, lineCountValue, codeLines, normalizedText]);

  const showMermaidFallback = mermaidMode && !shouldUsePlainRenderer && !mermaidError && !mermaidSvg;
  const shouldDisplayMermaid = mermaidMode && !shouldUsePlainRenderer && expanded && mermaidSvg !== null;
  const suppressMermaidErrorOutput = mermaidMode && !shouldUsePlainRenderer && mermaidError !== null && mermaidSvg === null;
  const needsFullscreenSafeTop = !fullscreenBounds || fullscreenBounds.top <= 0;
  const fullscreenHostStyle = fullscreenBounds
    ? {
      top: `${fullscreenBounds.top}px`,
      left: `${fullscreenBounds.left}px`,
      width: `${fullscreenBounds.width}px`,
      height: `${fullscreenBounds.height}px`,
    }
    : undefined;

  const resetMermaidView = useCallback(() => {
    mermaidPointerRef.current.clear();
    mermaidGestureRef.current = null;
    setMermaidPan({ x: 0, y: 0 });
    setMermaidScale(1);
  }, []);

  const openFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    if (mermaidMode) {
      resetMermaidView();
    }
    setExpanded(true);
    const host = findScrollableContainer(blockRef.current);
    scrollHostRef.current = host;
    if (host) {
      const rect = host.getBoundingClientRect();
      setFullscreenBounds({
        top: rect.top,
        left: rect.left,
        width: rect.width,
        height: rect.height,
      });
    } else {
      setFullscreenBounds(null);
    }
    setIsFullscreen(true);
  };

  const closeFullscreen = (event?: React.MouseEvent) => {
    event?.stopPropagation();
    if (mermaidMode) {
      resetMermaidView();
    }
    setIsFullscreen(false);
    setFullscreenBounds(null);
    scrollHostRef.current = null;
  };

  const startMermaidGesture = useCallback(() => {
    const points = Array.from(mermaidPointerRef.current.values());
    if (points.length >= 2) {
      const [first, second] = points;
      mermaidGestureRef.current = {
        distance: distanceBetween(first, second),
        midpoint: midpointBetween(first, second),
        pan: mermaidPan,
        scale: mermaidScale,
      };
      return;
    }
    if (points.length === 1) {
      mermaidGestureRef.current = {
        distance: 0,
        midpoint: points[0],
        pan: mermaidPan,
        scale: mermaidScale,
      };
      return;
    }
    mermaidGestureRef.current = null;
  }, [mermaidPan, mermaidScale]);

  const handleMermaidPointerDown = useCallback<React.PointerEventHandler<HTMLDivElement>>((event) => {
    if (!isFullscreen || !mermaidMode || !shouldDisplayMermaid) {
      return;
    }
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    mermaidPointerRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    startMermaidGesture();
  }, [isFullscreen, mermaidMode, shouldDisplayMermaid, startMermaidGesture]);

  const handleMermaidPointerMove = useCallback<React.PointerEventHandler<HTMLDivElement>>((event) => {
    if (!isFullscreen || !mermaidMode || !mermaidPointerRef.current.has(event.pointerId)) {
      return;
    }
    event.preventDefault();
    mermaidPointerRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const gesture = mermaidGestureRef.current;
    const points = Array.from(mermaidPointerRef.current.values());
    if (!gesture || points.length === 0) {
      return;
    }

    if (points.length >= 2 && gesture.distance > 0) {
      const [first, second] = points;
      const distance = distanceBetween(first, second);
      const midpoint = midpointBetween(first, second);
      setMermaidScale(clamp(
        gesture.scale * (distance / gesture.distance),
        MERMAID_FULLSCREEN_MIN_SCALE,
        MERMAID_FULLSCREEN_MAX_SCALE,
      ));
      setMermaidPan({
        x: gesture.pan.x + midpoint.x - gesture.midpoint.x,
        y: gesture.pan.y + midpoint.y - gesture.midpoint.y,
      });
      return;
    }

    const point = points[0];
    setMermaidPan({
      x: gesture.pan.x + point.x - gesture.midpoint.x,
      y: gesture.pan.y + point.y - gesture.midpoint.y,
    });
  }, [isFullscreen, mermaidMode]);

  const handleMermaidPointerUp = useCallback<React.PointerEventHandler<HTMLDivElement>>((event) => {
    if (!mermaidPointerRef.current.has(event.pointerId)) {
      return;
    }
    event.preventDefault();
    mermaidPointerRef.current.delete(event.pointerId);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    startMermaidGesture();
  }, [startMermaidGesture]);

  const handleMermaidWheel = useCallback<React.WheelEventHandler<HTMLDivElement>>((event) => {
    if (!isFullscreen || !mermaidMode || !shouldDisplayMermaid) {
      return;
    }
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    const originX = event.clientX - rect.left - rect.width / 2;
    const originY = event.clientY - rect.top - rect.height / 2;
    const direction = event.deltaY < 0 ? 1.12 : 0.88;
    setMermaidScale((currentScale) => {
      const nextScale = clamp(currentScale * direction, MERMAID_FULLSCREEN_MIN_SCALE, MERMAID_FULLSCREEN_MAX_SCALE);
      const ratio = nextScale / currentScale;
      setMermaidPan((currentPan) => ({
        x: originX - (originX - currentPan.x) * ratio,
        y: originY - (originY - currentPan.y) * ratio,
      }));
      return nextScale;
    });
  }, [isFullscreen, mermaidMode, shouldDisplayMermaid]);

  const fullscreenHeaderTopPadding = needsFullscreenSafeTop
    ? "calc(0.5rem + var(--ct-safe-top, 0px))"
    : undefined;

  return (
    <div ref={blockRef}>
      <div className="overflow-hidden rounded-md border border-[var(--ct-border)]" style={{ background: "var(--ct-bg)" }}>
        <div className="flex items-center gap-1.5 px-2 py-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
          <span className="font-medium" style={{ color: "var(--ct-fg)" }}>Code</span>
          <span className="font-mono opacity-70">{language}</span>
          <span className="opacity-70">·</span>
          <span className="opacity-70">{lineCountValue} lines</span>
          <span className="ml-auto" />
          {canCollapse && (
            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                setExpanded((value) => !value);
              }}
              className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
              style={{ color: "var(--ct-muted-fg)" }}
            >
              <ChevronIcon expanded={expanded} />
              <span>{expanded ? "Hide" : "Show all"}</span>
            </button>
          )}
          <button
            type="button"
            onClick={copyText}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            {copied ? <Check size={12} /> : <Copy size={12} />}
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
          <button
            type="button"
            onClick={openFullscreen}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
            style={{ color: "var(--ct-muted-fg)" }}
            title="Open in fullscreen"
          >
            <Maximize2 size={12} />
            <span>Fullscreen</span>
          </button>
        </div>
        <CodeBlockContent
          fullSize={false}
          maxHeight={maxHeight}
          displayedText={displayedText}
          language={language}
          mermaidMode={mermaidMode}
          mermaidSvg={mermaidSvg}
          forcePlainText={shouldUsePlainRenderer}
          shouldDisplayMermaid={shouldDisplayMermaid}
          showMermaidFallback={showMermaidFallback}
          suppressMermaidErrorOutput={suppressMermaidErrorOutput}
        />
        {showMermaidFallback && !mermaidError && mermaidLoading && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60 text-[var(--ct-muted-fg)]">Rendering mermaid diagram...</div>
        )}
        {canCollapse && isCollapsed && !mermaidMode && !mermaidLoading && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60" style={{ color: "var(--ct-muted-fg)" }}>
            Showing preview of a long block. Expand to see all lines.
          </div>
        )}
        {canCollapse && isCollapsed && mermaidMode && (
          <div className="px-2 pb-1.5 text-[10px] opacity-60" style={{ color: "var(--ct-muted-fg)" }}>
            Showing first and last lines only. Expand to render complete diagram.
          </div>
        )}
      </div>
      {isFullscreen && (
        <div
          className="fixed z-50 bg-black/85 backdrop-blur-sm"
          style={fullscreenHostStyle ?? { inset: "0px" }}
          onClick={closeFullscreen}
        >
          <div
            className="mx-auto flex h-full w-full flex-col border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 shadow-2xl"
            onClick={(event) => event.stopPropagation()}
          >
            <div
              className="flex items-center gap-1.5 px-2 py-1.5 text-xs"
              style={{
                color: "var(--ct-muted-fg)",
                paddingTop: fullscreenHeaderTopPadding,
              }}
            >
              <span className="font-medium" style={{ color: "var(--ct-fg)" }}>Code</span>
              <span className="font-mono opacity-70">{language}</span>
              <span className="opacity-70">·</span>
              <span className="opacity-70">{lineCountValue} lines</span>
              <span className="ml-auto" />
              <button
                type="button"
                onClick={copyText}
                className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
                style={{ color: "var(--ct-muted-fg)" }}
              >
                {copied ? <Check size={12} /> : <Copy size={12} />}
                <span>{copied ? "Copied" : "Copy"}</span>
              </button>
              <button
                type="button"
                onClick={closeFullscreen}
                className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-150 hover:brightness-110"
                style={{ color: "var(--ct-muted-fg)" }}
              >
                <Minimize2 size={12} />
                <span>Close</span>
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-hidden">
              <CodeBlockContent
                fullSize
                maxHeight={maxHeight}
                displayedText={displayedText}
                language={language}
                mermaidMode={mermaidMode}
                mermaidSvg={mermaidSvg}
                forcePlainText={shouldUsePlainRenderer}
                shouldDisplayMermaid={shouldDisplayMermaid}
                showMermaidFallback={showMermaidFallback}
                suppressMermaidErrorOutput={suppressMermaidErrorOutput}
                interactiveMermaid={mermaidMode}
                mermaidPan={mermaidPan}
                mermaidScale={mermaidScale}
                onMermaidPointerDown={handleMermaidPointerDown}
                onMermaidPointerMove={handleMermaidPointerMove}
                onMermaidPointerUp={handleMermaidPointerUp}
                onMermaidWheel={handleMermaidWheel}
                onMermaidDoubleClick={resetMermaidView}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Chevron icon for collapsible sections. */
function ChevronIcon({ expanded, className = "" }: { expanded: boolean; className?: string }) {
  return (
    <ChevronDown
      className={`w-3 h-3 flex-shrink-0 transition-transform duration-150 ${expanded ? "rotate-180" : ""} ${className}`}
      strokeWidth={1.5}
    />
  );
}

/** File icon SVG for attachment badges. */
function FileIcon({ className = "" }: { className?: string }) {
  return <File className={className} size={12} strokeWidth={2} />;
}

/** Extract filename from path. */
function fileName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Render user message content, replacing file attachment markers with badges.
 * Marker format: \x00«file:/path/to/file» — uses NUL + guillemets to avoid
 * false positives with any user-typed text, markdown, or code.
 */
interface ParsedVoice {
  transcript: string | null;
  note: string | null;
  fileUrl: string | null;
  fileName: string | null;
}

/** Parse the CodeTerm voice marker so the chat can show a voice card instead
 *  of the raw marker string. Returns null when the text isn't a voice line. */
function parseVoiceMessage(content: string): ParsedVoice | null {
  const m = parseMarker(content.trim());
  if (!m || m.kind !== "voice") return null;
  const fileUrl = m.attrs.file ?? null;
  const untranscribed = m.attrs.status === "untranscribed";
  const payload = m.payload.trim() || null;
  return {
    transcript: untranscribed ? null : payload,
    note: untranscribed ? payload : null,
    fileUrl,
    fileName: fileUrl ? fileName(fileUrl) : null,
  };
}

function VoiceCard({ v }: { v: ParsedVoice }) {
  return (
    <div className="flex flex-col gap-1">
      <div
        className="inline-flex items-center gap-1.5 text-[11px]"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        <Mic size={13} strokeWidth={2} />
        <span className="opacity-80 font-medium">Voice</span>
        {v.fileName && <span className="opacity-40 font-mono">{v.fileName}</span>}
      </div>
      {v.transcript ? (
        <pre className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{v.transcript}</pre>
      ) : (
        <span className="text-[11px] italic" style={{ color: "var(--ct-muted-fg)" }}>
          {v.note ?? "voice note"}
        </span>
      )}
    </div>
  );
}

function renderUserContent(content: string): React.ReactNode {
  const voice = parseVoiceMessage(content);
  if (voice) return <VoiceCard v={voice} />;
  // Non-voice attachment markers (photo/document/video/file) → file badge.
  const mk = parseMarker(content.trim());
  if (mk && ["photo", "file", "document", "video", "audio", "sticker"].includes(mk.kind)) {
    const fp = mk.attrs.file ?? "";
    const name = mk.attrs.name || (fp ? fileName(fp) : mk.kind);
    return (
      <div
        className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px]"
        style={{
          background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 12%, transparent)",
          color: "var(--ct-fg)",
        }}
        title={fp}
      >
        <FileIcon />
        <span className="font-medium">{name}</span>
        <span className="opacity-50 uppercase">{mk.kind}</span>
      </div>
    );
  }
  // Match both new marker (\x00«file:...») and legacy [attached: ...] format
  const attachRe = /(?:\x00\u00ABfile:([^\u00BB]+)\u00BB|\[attached:\s*([^\]]+)\])/g;
  const parts: React.ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = attachRe.exec(content)) !== null) {
    // Text before the match
    const before = content.slice(lastIndex, match.index).trim();
    if (before) {
      parts.push(<pre key={key++} className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{before}</pre>);
    }
    // File badge — group 1 = new marker, group 2 = legacy format
    const filePath = (match[1] || match[2]).trim();
    const name = fileName(filePath);
    const ext = name.split(".").pop()?.toUpperCase() || "FILE";
    parts.push(
      <div
        key={key++}
        className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 mt-1 text-[11px]"
        style={{
          background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 12%, transparent)",
          color: "var(--ct-fg)",
        }}
        title={filePath}
      >
        <FileIcon />
        <span className="font-medium">{name}</span>
        <span className="opacity-50">{ext}</span>
      </div>,
    );
    lastIndex = match.index + match[0].length;
  }

  // Remaining text
  const rest = content.slice(lastIndex).trim();
  if (rest) {
    parts.push(<pre key={key++} className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{rest}</pre>);
  }

  // No attachments found — plain text
  if (parts.length === 0) {
    return <pre className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{content}</pre>;
  }

  return <>{parts}</>;
}

/** Compact image indicator for standalone image messages. */
function ImageBadge() {
  return (
    <span
      className="inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs"
      style={{ background: "var(--ct-surface)", color: "var(--ct-muted-fg)" }}
    >
      <Image size={14} strokeWidth={2} />
      <span className="opacity-70">Image</span>
    </span>
  );
}

/** Renders slash-command blocks (name chip, optional args/stdout). */
function CommandBody({ commands, body }: { commands: SlashCommand[]; body: string }) {
  return (
    <div className="space-y-2">
      {commands.map((c, i) => (
        <div key={i} className="space-y-1">
          <div className="flex items-center gap-2">
            <code
              className="px-2 py-0.5 rounded text-xs font-mono"
              style={{
                background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)",
                color: "var(--ct-accent, #58a6ff)",
              }}
            >
              {c.name}
            </code>
            {c.args && (
              <span className="text-xs opacity-70 font-mono">{c.args}</span>
            )}
          </div>
          {c.stdout && (
            <pre
              className={`text-xs font-mono px-2 py-1.5 rounded ${PRE_WRAP_CLASS}`}
              style={{
                background: "color-mix(in srgb, var(--ct-bg, #0d1117) 70%, black 5%)",
                color: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 80%, transparent)",
                borderLeft: "2px solid color-mix(in srgb, var(--ct-muted-fg) 40%, transparent)",
              }}
            >
              {c.stdout}
            </pre>
          )}
        </div>
      ))}
      {body.trim() && (
        <div className="text-sm opacity-70">{body}</div>
      )}
    </div>
  );
}

// Model family → avatar glyph + accent (self-contained; no provider registry).
function modelAvatar(model?: string): { glyph: string; color: string } {
  const m = (model ?? "").toLowerCase();
  if (m.includes("opus")) return { glyph: "Op", color: "#d2a24c" };   // amber
  if (m.includes("sonnet")) return { glyph: "So", color: "#5b9bf5" }; // blue
  if (m.includes("haiku")) return { glyph: "Ha", color: "#4ec98c" };  // green
  if (m.includes("codex") || m.includes("gpt") || m.includes("o1") || m.includes("o3"))
    return { glyph: "Cx", color: "#b07ce8" };                         // violet
  if (m.includes("gemini")) return { glyph: "Ge", color: "#e88c5a" }; // orange
  const initial = (model ?? "").replace(/[^a-z0-9]/gi, "").slice(0, 2);
  return { glyph: initial ? initial[0].toUpperCase() + (initial[1] ?? "") : "·", color: "#8b949e" };
}

function ModelAvatar({ model }: { model?: string }) {
  const { glyph, color } = modelAvatar(model);
  return (
    <span
      aria-hidden
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 18,
        height: 18,
        borderRadius: "50%",
        fontSize: 8.5,
        fontWeight: 700,
        fontFamily: "var(--ct-mono, ui-monospace, monospace)",
        letterSpacing: "-0.5px",
        color,
        background: `color-mix(in srgb, ${color} 16%, transparent)`,
        border: `1px solid color-mix(in srgb, ${color} 45%, transparent)`,
        flexShrink: 0,
      }}
    >
      {glyph}
    </span>
  );
}

interface MessageBubbleProps {
  message: ChatMessage;
  /** When set, this user message arrived via a codeterm channel (cross-pane,
   *  telegram, mesh, slash-command) and should render as a typed header
   *  instead of the default right-aligned "user" styling. */
  origin?: ChatOrigin;
  originAvatar?: React.ReactNode;
  /** When set, clicking the origin header focuses the source pane. */
  onOriginClick?: () => void;
}

export const MessageBubble = React.memo(function MessageBubble({ message, origin, originAvatar, onOriginClick }: MessageBubbleProps) {
  // Auto-expand for Write-style tool input (the file body being written) and
  // for Edit/MultiEdit (always show the diff). A bash/grep result must NOT
  // force-expand — it shows a compact inline tail under the pill instead.
  const shouldAutoExpand = message.type === "tool_call"
    && (message.toolInput?.content != null || extractEdits(message).length > 0);
  const [expanded, setExpanded] = useState(shouldAutoExpand);
  const [viewingImage, setViewingImage] = useState<ImageAttachment | null>(null);

  const renderedContent = useMemo(() => {
    if (message.type === "assistant") {
      return <RichMarkdown text={message.content} />;
    }
    return null;
  }, [message.type, message.content]);

  const handleImageClick = useCallback((att: ImageAttachment) => {
    setViewingImage(att);
  }, []);

  useEffect(() => {
    if (shouldAutoExpand) setExpanded(true);
  }, [shouldAutoExpand]);

  // Image message (standalone, legacy) — compact placeholder
  if (message.type === "image") {
    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <ImageBadge />
      </div>
    );
  }

  // User message — right-aligned by default; left-aligned with origin header for injected messages
  if (message.type === "user") {
    const attachments = message.attachments;

    // Origin header rendered above the bubble when the message came from a
    // codeterm channel (cross-pane, telegram, mesh) or wraps slash-commands.
    const originHeader = origin ? (() => {
      // Cross-pane / report: model-avatar + "from <Name>" + mesh·model subtitle.
      if (origin.kind === "pane") {
        const LeadIcon = origin.report ? CornerUpLeft : ArrowRight;
        const subtitle = [origin.mesh ? `via ${origin.mesh}` : null, origin.model]
          .filter(Boolean)
          .join(" · ");
        return (
          <button
            type="button"
            onClick={onOriginClick}
            disabled={!onOriginClick}
            className="flex items-center gap-1.5 mb-1"
            style={{
              cursor: onOriginClick ? "pointer" : "default",
              background: "transparent",
              border: "none",
              padding: 0,
              textAlign: "left",
            }}
            title={onOriginClick ? "Click to focus source pane" : undefined}
          >
            {originAvatar ?? <ModelAvatar model={origin.model} />}
            <span className="flex flex-col min-w-0" style={{ lineHeight: 1.1 }}>
              <span
                className="flex items-center gap-1 text-xs uppercase tracking-wider"
                style={{ color: "var(--ct-accent, #58a6ff)" }}
              >
                <LeadIcon size={11} style={{ opacity: 0.85 }} />
                <span className="truncate">
                  {origin.report ? "report · " : "from "}
                  {origin.title}
                </span>
              </span>
              {subtitle && (
                <span
                  className="text-[10px] truncate"
                  style={{ color: "var(--ct-muted-fg, #8b949e)", textTransform: "none", letterSpacing: 0 }}
                >
                  {subtitle}
                </span>
              )}
            </span>
          </button>
        );
      }

      const OriginIcon =
        origin.kind === "mesh"    ? Network :
        origin.kind === "command" ? ChevronsRight :
        Send;
      const label =
        origin.kind === "telegram"? `Telegram · ${origin.userId.slice(0, 12)}` :
        origin.kind === "mesh"    ? `Mesh · ${origin.peerId.slice(0, 10)}` :
        origin.kind === "command" ? (
          origin.commands.length === 1
            ? origin.commands[0].name
            : `${origin.commands.length} commands`
        ) :
        `${origin.service} · ${origin.userId.slice(0, 12)}`;
      return (
        <button
          type="button"
          onClick={onOriginClick}
          disabled={!onOriginClick}
          className="flex items-center gap-1.5 text-xs mb-1 uppercase tracking-wider opacity-80"
          style={{
            color: "var(--ct-accent, #58a6ff)",
            cursor: onOriginClick ? "pointer" : "default",
            background: "transparent",
            border: "none",
            padding: 0,
            textTransform: "uppercase",
          }}
          title={onOriginClick ? "Click to focus source pane" : undefined}
        >
          <OriginIcon size={12} />
          <span>{label}</span>
        </button>
      );
    })() : null;

    return (
      <div className={`flex mb-3 w-full max-w-4xl min-w-0 mx-auto ${origin ? "justify-start" : "justify-end"}`}>
        <div className="min-w-0 max-w-[75%] sm:max-w-[75%]">
          {originHeader}
          <div
            className={`min-w-0 rounded-2xl rounded-br-sm px-4 py-2 text-sm select-text cursor-text border ${TEXT_WRAP_CLASS}`}
            style={{
              // The user voice is the ONE filled element in the feed — a clear accent
              // tint so it never blends with tool traces or assistant prose.
              background: origin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, var(--ct-surface, #161b22) 88%)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, var(--ct-bg, #0d1117) 78%)",
              borderColor: origin
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)"
                : "color-mix(in srgb, var(--ct-accent, #58a6ff) 45%, transparent)",
              color: "var(--ct-fg)",
            }}
          >
            {origin?.kind === "command" ? (
              <CommandBody commands={origin.commands} body={message.content} />
            ) : (
              renderUserContent(message.content)
            )}
            {attachments && attachments.length > 0 && (
              <div className="flex flex-wrap gap-1.5 mt-2 pt-2" style={{ borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}>
                {attachments.map((att) => (
                  <button
                    key={att.id}
                    onClick={() => handleImageClick(att)}
                    className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors hover:brightness-125"
                    style={{
                      background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)",
                      color: "var(--ct-muted-fg)",
                    }}
                    title={att.data ? "Click to preview" : "Image (not loaded)"}
                  >
                    <Image size={12} strokeWidth={2} />
                    {att.mimeType.replace("image/", "").toUpperCase()}
                  </button>
                ))}
              </div>
            )}
            {/* Inline image preview */}
            {viewingImage && viewingImage.data && (
              <div className="mt-2 pt-2" style={{ borderTop: "1px solid color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}>
                <img
                  src={`data:${viewingImage.mimeType};base64,${viewingImage.data}`}
                  alt="Attached image"
                  className="max-w-full max-h-[300px] object-contain rounded-lg"
                />
                <div className="flex items-center gap-2 mt-1.5">
                  <a
                    href={`data:${viewingImage.mimeType};base64,${viewingImage.data}`}
                    download={`image.${viewingImage.mimeType.replace("image/", "")}`}
                    className="inline-flex items-center gap-1 text-[11px] px-1.5 py-0.5 rounded transition-colors hover:brightness-125"
                    style={{ color: "var(--ct-muted-fg)", background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}
                  >
                    <Download size={11} strokeWidth={2} />
                    Save
                  </a>
                  <button
                    onClick={() => setViewingImage(null)}
                    className="text-[11px] px-1.5 py-0.5 rounded transition-colors hover:brightness-125"
                    style={{ color: "var(--ct-muted-fg)" }}
                  >
                    Hide
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  // Assistant message — left-aligned, NO bubble, clean prose
  if (message.type === "assistant") {
    return (
      <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
        <div
          className={`min-w-0 max-w-full text-sm leading-relaxed select-text cursor-text ${TEXT_WRAP_CLASS}`}
          style={{ color: "var(--ct-fg)" }}
        >
          {renderedContent}
        </div>
      </div>
    );
  }

  if (message.type === "system" || message.type === "local_command") {
    const label = message.tagName || message.systemSubtype || "system";
    const isCaveat = message.tagName === "local-command-caveat";
    return (
      <div className="mb-2 w-full max-w-4xl min-w-0 mx-auto">
        <div
          className="inline-flex max-w-full min-w-0 flex-col gap-1 rounded-md px-2.5 py-1.5 text-xs border"
          style={{
            background: isCaveat
              ? "color-mix(in srgb, var(--ct-yellow, #d29922) 7%, var(--ct-surface, #161b22) 93%)"
              : "var(--ct-surface)",
            borderColor: isCaveat
              ? "color-mix(in srgb, var(--ct-yellow, #d29922) 25%, transparent)"
              : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
            color: "var(--ct-muted-fg)",
          }}
        >
          <span
            className="inline-flex items-center gap-1.5 font-mono text-[11px]"
            style={{ color: isCaveat ? "var(--ct-yellow, #d29922)" : "var(--ct-accent, #58a6ff)" }}
          >
            <Info size={12} strokeWidth={2} />
            {label}
          </span>
          {message.content && (
            <pre className={`${PRE_WRAP_CLASS} font-mono m-0 select-text`} style={{ color: "var(--ct-fg)" }}>
              {message.content}
            </pre>
          )}
        </div>
      </div>
    );
  }

  // Tool call — compact pill/card
  if (message.type === "tool_call") {
    const toolName = message.toolName || "";
    const filePath = extractFilePath(message);
    const command = extractCommand(message);
    const hasToolContent = message.toolInput?.content != null;
    const toolContent = hasToolContent ? String(message.toolInput!.content) : null;
    const edits = extractEdits(message);
    const addedLines = edits.reduce((n, e) => n + (e.new ? e.new.split("\n").length : 0), 0);
    const removedLines = edits.reduce((n, e) => n + (e.old ? e.old.split("\n").length : 0), 0);

    // Build summary text
    let summary = "";
    if (filePath) {
      summary = shortPath(filePath);
    } else if (command) {
      const firstLine = command.split("\n")[0];
      summary = firstLine.length > 60 ? firstLine.slice(0, 60) + "..." : firstLine;
    } else if (message.toolArgs) {
      const firstLine = message.toolArgs.split("\n")[0].trim();
      summary = firstLine.length > 60 ? firstLine.slice(0, 60) + "..." : firstLine;
    }

    // TodoWrite — render checklist inline
    if (message.toolName === "TodoWrite" && Array.isArray(message.toolInput?.todos)) {
      const todos = message.toolInput!.todos as Array<{ content: string; status: string; id?: string }>;
      const STATUS_ICON: Record<string, string> = { pending: "⏳", in_progress: "🚧", completed: "✅" };
      return (
        <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
          <div className="inline-flex max-w-full min-w-0 flex-col gap-0.5 rounded-md px-2.5 py-1.5" style={{ background: "var(--ct-surface)" }}>
            <span className="text-[10px] font-medium mb-0.5" style={{ color: "var(--ct-muted-fg)" }}>TodoWrite</span>
            {todos.map((t, i) => (
              <div key={t.id ?? i} className="flex items-start gap-1.5 text-xs" style={{ color: "var(--ct-fg)" }}>
                <span className="flex-shrink-0">{STATUS_ICON[t.status] ?? "⏳"}</span>
                <span className={t.status === "completed" ? "opacity-50 line-through" : ""}>{t.content}</span>
              </div>
            ))}
          </div>
        </div>
      );
    }

    // CodeTerm CLI command — render a branded semantic card across Claude/Codex
    // shell tool shapes.
    const shellCommand = command;
    if (shellCommand) {
      const inv = recognize(shellCommand);
      if (inv) {
        return (
          <CodetermCard
            inv={inv}
            result={message.toolResult}
            expanded={expanded}
            onToggle={() => setExpanded(!expanded)}
          />
        );
      }
    }

    const detailText = message.toolArgs || message.content;
    const codeLanguage = languageFromPath(filePath);
    const resultText = message.toolResult;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
          <button
            onClick={() => setExpanded(!expanded)}
            // De-bubbled on purpose: tool activity is a thin terminal trace (hairline border,
            // no fill, green exec glyph) so it visually recedes behind user/assistant voices.
            className="inline-flex min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
            style={{
              background: "transparent",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
              color: "var(--ct-muted-fg)",
            }}
          >
            <span
              className="flex-shrink-0"
              style={{ color: "color-mix(in srgb, var(--ct-green, #3fb950) 75%, var(--ct-fg, #c9d1d9) 25%)" }}
            >
              <ToolIcon name={toolName} />
            </span>
            <span className="font-medium font-mono" style={{ color: "var(--ct-muted-fg)" }}>
              {toolName === "shell_command" ? "shell" : message.toolName}
            </span>
            {summary && (
              <span className="min-w-0 truncate opacity-60 font-mono text-[11px]">{summary}</span>
            )}
            {toolContent && (
              <span className="shrink-0 opacity-40 font-mono text-[11px]">
                {lineCount(toolContent)} lines
              </span>
            )}
            {edits.length > 0 && (
              <span className="shrink-0 font-mono text-[11px]">
                <span style={{ color: "var(--ct-green, #3fb950)" }}>+{addedLines}</span>{" "}
                <span style={{ color: "var(--ct-red, #f85149)" }}>-{removedLines}</span>
              </span>
            )}
            <ChevronIcon expanded={expanded} className="ml-1 opacity-50" />
          </button>

          {/* Compact inline tail — last line of output, always visible while
              collapsed. Click to reveal the full result under the command. */}
          {resultText && !expanded && (
            <button
              onClick={() => setExpanded(true)}
              className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
              style={{ color: "var(--ct-muted-fg)" }}
              title="Show full output"
            >
              <span className="flex-shrink-0 opacity-40">{"→"}</span>
              <span className="min-w-0 truncate opacity-70">{lastNonEmptyLine(resultText)}</span>
            </button>
          )}

          {expanded && (
            <div
              className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2 text-xs"
              style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
            >
              {toolContent ? (
                <CodeBlock text={toolContent} language={codeLanguage} maxHeight="24rem" />
              ) : edits.length > 0 ? (
                <DiffView edits={edits} />
              ) : (
                <pre
                  className={`${PRE_WRAP_CLASS} m-0 font-mono`}
                  style={{ color: "var(--ct-fg)" }}
                >
                  {detailText}
                </pre>
              )}
              {resultText && (
                <div className="mt-2">
                  <div
                    className="mb-1 text-[10px] font-medium uppercase tracking-wide"
                    style={{ color: "var(--ct-muted-fg)" }}
                  >
                    Result
                  </div>
                  <PlainOutput text={resultText} maxHeight="12rem" />
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  // Tool result — subtle, compact
  if (message.type === "tool_result") {
    const contentLines = message.content.split("\n");
    const preview = contentLines.slice(0, 2).join("\n");
    const hasMore = contentLines.length > 2;

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto pl-4">
        <button
          onClick={() => setExpanded(!expanded)}
          className="inline-flex max-w-full min-w-0 items-start gap-1.5 text-left rounded-md px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110 sm:max-w-md"
          style={{
            color: "var(--ct-muted-fg)",
          }}
        >
          <span className="opacity-40 flex-shrink-0 mt-0.5 text-[10px]">{"\u2192"}</span>
          <span
            className="opacity-60 font-mono text-[11px] overflow-hidden"
            style={{ display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" as const }}
          >
            {preview}
            {hasMore && !expanded ? "..." : ""}
          </span>
          {hasMore && <ChevronIcon expanded={expanded} className="ml-1 mt-0.5 opacity-40" />}
        </button>
        {expanded && (
          <div
            className="mt-1 ml-4 max-w-full min-w-0 rounded-md px-3 py-2 text-xs font-mono overflow-x-auto max-h-48 overflow-y-auto border border-[var(--ct-border)]"
            style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
          >
            <pre className={`${PRE_WRAP_CLASS} m-0`}>{message.content}</pre>
          </div>
        )}
      </div>
    );
  }

  // Thinking block — minimal, collapsed by default
  if (message.type === "thinking") {
    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <button
          onClick={() => setExpanded(!expanded)}
          className="inline-flex items-center gap-1.5 text-left rounded-md px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
          style={{
            color: "var(--ct-muted-fg)",
          }}
        >
          <span className="opacity-40 text-[10px]">{"..."}</span>
          <span className="opacity-50 text-[11px]">Thinking</span>
          <ChevronIcon expanded={expanded} className="opacity-40" />
        </button>
        {expanded && (
          <div
            className="mt-1 max-w-full min-w-0 rounded-md px-3 py-2 text-xs overflow-x-auto max-h-48 overflow-y-auto italic border border-[var(--ct-border)]"
            style={{
              background: "var(--ct-bg)",
              color: "var(--ct-muted-fg)",
            }}
          >
            <pre className={`${PRE_WRAP_CLASS} m-0 font-[inherit]`}>
              {message.content}
            </pre>
          </div>
        )}
      </div>
    );
  }

  // Status message
  if (message.type === "status") {
    return (
      <div className="flex w-full max-w-4xl min-w-0 justify-center mb-2 mx-auto">
        <span
          className="text-[10px] px-2.5 py-0.5 rounded-full"
          style={{
            color: "var(--ct-muted-fg)",
            background: "var(--ct-surface)",
          }}
        >
          {message.content}
        </span>
      </div>
    );
  }

  return null;
});
