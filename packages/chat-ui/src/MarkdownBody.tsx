import React, { useMemo } from "react";
import type { SlashCommand, EditPair } from "@codeterm/chat-core";
import { parseMarker } from "@codeterm/chat-core";
import { File, Image, Info, Mic, Settings } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import { CHAT_HTML_SANITIZE_SCHEMA, containsHtmlMarkup, normalizeMultilineHtmlBlocks, rehypeScrubInlineStyles } from "./htmlSanitize";
import { HtmlPreviewBlock } from "./HtmlPreviewBlock";
import { LocalArtifactBlock, parseLocalArtifactDirective } from "./LocalArtifactBlock";
import { Prism as SyntaxHighlighter, createElement } from "react-syntax-highlighter";
import { CodeBlock, ChevronIcon, CODE_BLOCK_DEFAULT_MAX_HEIGHT, LONG_CODE_PREVIEW_LINES, MERMAID_BLOCK_MAX_HEIGHT, PRE_WRAP_CLASS, lineCount } from "./CodeBlock";
import { looksLikeMermaidSource } from "./MermaidBlock";
import { themeAwarePrismStyle, SYNTAX_HIGHLIGHT_ROOT_CLASS } from "./syntaxTheme";
import { inferCodeLanguage } from "./codeLanguage";
import { pickUiFontScaleValue } from "./uiFontScale";
export { extractCommand, extractEdits, extractFilePath } from "./toolcard/toolParsing";
import { AttachedDiscussionDocView } from "./AttachedDiscussionDoc";
import { parseAttachedDiscussionDoc } from "./discussionDoc";
import { decodeCliMessageText } from "./codeterm/messageText";

export const TEXT_WRAP_CLASS = "break-words [overflow-wrap:anywhere]";

/** Above this many diff lines we skip per-token Prism highlighting and render
 *  plain (still colored +/- rows) — tokenizing a huge hunk is wasteful. */
export const DIFF_HIGHLIGHT_MAX_LINES = 400;
export const LONG_ARTIFACT_PREVIEW_LINES = 36;
export const LONG_ARTIFACT_PREVIEW_MAX_HEIGHT = 320;
export const LONG_ARTIFACT_FULL_MAX_HEIGHT = 700;

function extractCodeText(children: unknown): string {
  if (Array.isArray(children)) {
    return children.map((child) => (typeof child === "string" ? child : "")).join("");
  }
  return typeof children === "string" ? children : "";
}

function resolveCodeLanguage(className: string | undefined, children: unknown): string | null {
  const explicit = /language-([a-z0-9+#-]+)/i.exec(className || "");
  if (explicit) {
    return explicit[1].toLowerCase();
  }
  const source = extractCodeText(children).replace(/\r/g, "").trim();
  return looksLikeMermaidSource(source) ? "mermaid" : null;
}

function wrapsCodeElement(node: unknown): boolean {
  const children = (node as { children?: Array<{ type?: string; tagName?: string }> } | undefined)?.children;
  if (!children) return true;
  return children.some((child) => child.type === "element" && child.tagName === "code");
}

function isBlockCodeNode(node: unknown): boolean {
  if (!node || typeof node !== "object" || !("position" in node)) return false;
  const position = (node as { position?: { start?: { line?: number }; end?: { line?: number } } }).position;
  return position?.start?.line !== position?.end?.line;
}

function MarkdownCode({
  node,
  className,
  children,
  ...props
}: React.ComponentProps<"code"> & { node?: unknown }) {
  const language = resolveCodeLanguage(className, children);
  const plainCode = extractCodeText(children);
  const blockCode = isBlockCodeNode(node);
  if (!language && !blockCode) {
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
  if (!language) {
    return (
      <code
        data-testid="chat-plain-code-block"
        className="my-2 block max-w-full overflow-x-auto whitespace-pre rounded-md border p-3 font-mono text-xs leading-[1.4]"
        style={{
          background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 7%, transparent)",
          borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
          color: "var(--ct-fg, #c9d1d9)",
        }}
        {...props}
      >
        {plainCode}
      </code>
    );
  }
  if (language === "html") {
    return <HtmlPreviewBlock text={plainCode} />;
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
}

function MarkdownPre({
  node,
  children,
  ...props
}: React.ComponentProps<"pre"> & { node?: unknown }) {
  if (wrapsCodeElement(node)) {
    return <>{children}</>;
  }
  return (
    <pre
      className="my-2 max-w-full overflow-x-auto rounded-md border p-3 font-mono text-xs leading-[1.4]"
      style={{
        background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 7%, transparent)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
      {...props}
    >
      {children}
    </pre>
  );
}

type RehypePlugins = React.ComponentProps<typeof ReactMarkdown>["rehypePlugins"];

/** Raw-HTML parsing only pays for itself on messages that actually carry tags. */
const HTML_REHYPE_PLUGINS = [
  rehypeRaw,
  [rehypeSanitize, CHAT_HTML_SANITIZE_SCHEMA],
  rehypeScrubInlineStyles,
] as unknown as RehypePlugins;

export function RichMarkdown({
  text,
  fontScale,
  localArtifacts = true,
}: {
  text: string;
  fontScale?: number;
  localArtifacts?: boolean;
}) {
  const h1Text = pickUiFontScaleValue(fontScale, ["text-sm", "text-base", "text-lg", "text-xl"]);
  const h2Text = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const h3Text = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const renderedText = useMemo(() => normalizeMultilineHtmlBlocks(text), [text]);
  const rehypePlugins = useMemo(() => (containsHtmlMarkup(renderedText) ? HTML_REHYPE_PLUGINS : undefined), [renderedText]);
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={rehypePlugins}
      components={{
        h1: ({ node: _node, ...props }) => (
          <h1 className={`${h1Text} font-bold mt-3 mb-1`} style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        h2: ({ node: _node, ...props }) => (
          <h2 className={`${h2Text} font-semibold mt-2 mb-1`} style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        h3: ({ node: _node, ...props }) => (
          <h3 className={`${h3Text} font-medium mt-1.5 mb-0.5`} style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        p: ({ node: _node, children, ...props }) => {
          const textChild = React.Children.toArray(children).every((child) => typeof child === "string")
            ? React.Children.toArray(children).join("")
            : null;
          const directive = localArtifacts && textChild
            ? parseLocalArtifactDirective(textChild)
            : null;
          if (directive) {
            return (
              <LocalArtifactBlock
                directive={directive}
                renderMarkdown={(source) => (
                  <RichMarkdown text={source} fontScale={fontScale} localArtifacts={false} />
                )}
              />
            );
          }
          return <p className={`my-1 whitespace-pre-wrap ${TEXT_WRAP_CLASS}`} {...props}>{children}</p>;
        },
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
        code: MarkdownCode,
        pre: MarkdownPre,
        details: ({ node: _node, ...props }) => (
          <details
            className="my-2 rounded-md border px-2 py-1.5"
            style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 25%, transparent)" }}
            {...props}
          />
        ),
        summary: ({ node: _node, ...props }) => (
          <summary className="cursor-pointer select-none font-medium" style={{ color: "var(--ct-fg)" }} {...props} />
        ),
        kbd: ({ node: _node, ...props }) => (
          <kbd
            className="rounded border px-1 py-0.5 font-mono text-[0.85em]"
            style={{
              borderColor: "color-mix(in srgb, var(--ct-muted-fg) 35%, transparent)",
              background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 14%, transparent)",
            }}
            {...props}
          />
        ),
        mark: ({ node: _node, ...props }) => (
          <mark
            className="rounded px-0.5"
            style={{ background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 32%, transparent)", color: "inherit" }}
            {...props}
          />
        ),
        img: ({ node: _node, ...props }) => {
          const source = typeof props.src === "string" ? props.src.trim() : "";
          if (!localArtifacts && !/^data:image\/(?!svg\+xml)/i.test(source)) {
            return <span className="text-xs opacity-60">[external image omitted]</span>;
          }
          return <img className="my-1 inline-block max-w-full rounded" loading="lazy" {...props} />;
        },
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
      {renderedText}
    </ReactMarkdown>
  );
}

type DiffLineKind = "del" | "add";
interface DiffLine {
  kind: DiffLineKind;
  editIndex: number;
  content: string;
}

/** Flatten Edit/MultiEdit pairs into ordered del-then-add lines, tagged with
 *  their source edit so the renderer can keep per-edit separators. */
function buildDiffLines(edits: EditPair[]): DiffLine[] {
  const lines: DiffLine[] = [];
  edits.forEach((e, i) => {
    if (e.old) for (const l of e.old.split("\n")) lines.push({ kind: "del", editIndex: i, content: l });
    if (e.new) for (const l of e.new.split("\n")) lines.push({ kind: "add", editIndex: i, content: l });
  });
  return lines;
}

function diffRowStyle(kind: DiffLineKind): React.CSSProperties {
  return {
    background:
      kind === "del"
        ? "var(--ct-diff-del-bg, color-mix(in srgb, var(--ct-red, #f85149) 12%, var(--ct-bg, transparent)))"
        : "var(--ct-diff-add-bg, color-mix(in srgb, var(--ct-green, #3fb950) 12%, var(--ct-bg, transparent)))",
  };
}

function diffGutter(kind: DiffLineKind) {
  return (
    <span style={{ color: kind === "del" ? "var(--ct-diff-del-fg, var(--ct-red, #f85149))" : "var(--ct-diff-add-fg, var(--ct-green, #3fb950))" }}>
      {kind === "del" ? "- " : "+ "}
    </span>
  );
}

/** Old→new diff for Edit/MultiEdit tool calls: removed lines in red, added in
 *  green, with the edited file's syntax highlighting layered on top via the same
 *  Prism highlighter CodeBlock uses. Memoized so the hunk tokenizes once, not on
 *  every parent re-render; huge hunks fall back to plain rows (no token spans).
 *  Unknown languages degrade gracefully — Prism just renders untokenized text. */
export const DiffView = React.memo(function DiffView({
  edits,
  language = "text",
}: {
  edits: EditPair[];
  language?: string;
}) {
  const lines = useMemo(() => buildDiffLines(edits), [edits]);
  const combinedText = useMemo(() => lines.map((l) => l.content).join("\n"), [lines]);
  const resolvedLanguage = useMemo(
    () => (language !== "text" ? language : inferCodeLanguage(null, combinedText)),
    [language, combinedText],
  );
  const highlight = lines.length > 0 && lines.length <= DIFF_HIGHLIGHT_MAX_LINES;

  const separatorBefore = (i: number) => i > 0 && lines[i].editIndex !== lines[i - 1].editIndex;

  const plainRow = (line: DiffLine, i: number) => (
    <React.Fragment key={i}>
      {separatorBefore(i) && <div className="mt-2 border-t border-[var(--ct-border)] pt-2" />}
      <div className={`${PRE_WRAP_CLASS} px-2 py-0.5`} style={diffRowStyle(line.kind)}>
        {diffGutter(line.kind)}
        {line.content || " "}
      </div>
    </React.Fragment>
  );

  return (
    <div className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} font-mono text-[11px] leading-relaxed overflow-x-auto rounded-md`}>
      {highlight ? (
        <SyntaxHighlighter
          language={resolvedLanguage}
          style={themeAwarePrismStyle}
          useInlineStyles={false}
          PreTag="div"
          CodeTag="div"
          customStyle={{
            margin: 0,
            padding: 0,
            background: "transparent",
            fontSize: "inherit",
            lineHeight: "inherit",
            overflowWrap: "anywhere",
            wordBreak: "break-word",
          }}
          codeTagProps={{ style: { fontFamily: "inherit" } }}
          renderer={({ rows, stylesheet, useInlineStyles }) =>
            lines.map((line, i) => {
              const node = rows[i];
              return (
                <React.Fragment key={i}>
                  {separatorBefore(i) && <div className="mt-2 border-t border-[var(--ct-border)] pt-2" />}
                  <div className={`${PRE_WRAP_CLASS} px-2 py-0.5`} style={diffRowStyle(line.kind)}>
                    {diffGutter(line.kind)}
                    {node
                      ? createElement({ node, stylesheet, useInlineStyles, key: `c${i}` })
                      : line.content || " "}
                  </div>
                </React.Fragment>
              );
            })
          }
        >
          {combinedText}
        </SyntaxHighlighter>
      ) : (
        lines.map(plainRow)
      )}
    </div>
  );
});

/** Truncate file path to show just filename + parent. */
export function shortPath(fullPath: string): string {
  const parts = fullPath.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 2) return fullPath;
  return ".../" + parts.slice(-2).join("/");
}

/** Extract shell command text across Claude/Codex provider shapes. */
export function PlainOutput({ text, maxHeight = "12rem" }: { text: string; maxHeight?: string }) {
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

export function isLargeArtifact(text: string): boolean {
  return text.length > 2_500 || lineCount(text) > 45;
}

function artifactPreview(text: string): string {
  const lines = text.replace(/\r/g, "").split("\n").map((l) => l.trim()).filter(Boolean);
  return lines.slice(0, 2).join(" · ").slice(0, 220) || "output";
}

export function longArtifactContentPreview(text: string): string {
  return text.replace(/\r/g, "").split("\n").slice(0, LONG_ARTIFACT_PREVIEW_LINES).join("\n").slice(0, 6_000);
}

export function LongArtifact({
  label,
  text,
  expanded,
  onToggle,
  markdown = false,
}: {
  label: string;
  text: string;
  expanded: boolean;
  onToggle: () => void;
  markdown?: boolean;
}) {
  const lines = lineCount(text);
  const hasFullView = isLargeArtifact(text);
  const [showAll, setShowAll] = React.useState(false);
  React.useEffect(() => setShowAll(false), [expanded, text]);
  const visibleText = hasFullView && !showAll ? longArtifactContentPreview(text) : text;
  return (
    <div className="mb-2 w-full max-w-4xl min-w-0 mx-auto">
      <div
        className="max-w-full min-w-0 rounded-md border"
        style={{
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, transparent)",
          borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
        }}
      >
        <button
          type="button"
          onClick={onToggle}
          className="flex w-full min-w-0 items-center gap-2 px-2.5 py-1.5 text-left text-xs"
          style={{
            color: "var(--ct-muted-fg)",
            background: "transparent",
            border: "none",
          }}
        >
          <Info size={12} strokeWidth={2} className="shrink-0" style={{ color: "var(--ct-accent, #58a6ff)" }} />
          <span className="shrink-0 font-mono text-[11px]" style={{ color: "var(--ct-accent, #58a6ff)" }}>
            {label}
          </span>
          <span className="min-w-0 flex-1 truncate opacity-70">{artifactPreview(text)}</span>
          <span className="shrink-0 font-mono text-[10px] opacity-50">{lines} lines</span>
          <ChevronIcon expanded={expanded} className="shrink-0 opacity-50" />
        </button>
        {expanded && (
          <div className="border-t" style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 16%, transparent)" }}>
            <div
              data-testid="long-artifact-content"
              role={showAll ? "region" : undefined}
              aria-label={showAll ? `${label} full content` : undefined}
              tabIndex={showAll ? 0 : undefined}
              className="px-3 py-2 text-xs"
              style={{
                color: "var(--ct-fg)",
                maxHeight: `${showAll ? LONG_ARTIFACT_FULL_MAX_HEIGHT : LONG_ARTIFACT_PREVIEW_MAX_HEIGHT}px`,
                overflowY: showAll ? "auto" : "hidden",
                overflowX: "auto",
              }}
            >
              {markdown ? (
                <div className="text-sm leading-relaxed">
                  <RichMarkdown text={visibleText} />
                </div>
              ) : (
                <pre className={`${PRE_WRAP_CLASS} m-0 font-mono`}>{visibleText}</pre>
              )}
            </div>
            {hasFullView && (
              <div className="flex justify-center border-t px-3 py-1.5" style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 12%, transparent)" }}>
                <button
                  type="button"
                  data-testid="long-artifact-show-all"
                  className="rounded px-2 py-1 text-[11px]"
                  style={{ color: "var(--ct-accent)", background: "var(--ct-hover)" }}
                  onClick={() => setShowAll((current) => !current)}
                >
                  {showAll ? "Show preview" : "Show all"}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export { inferCodeLanguage, languageForEdits, languageFromPath } from "./codeLanguage";

/** Last non-empty line of a tool result — used for the compact inline tail. */
export function lastNonEmptyLine(text: string): string {
  const lines = text.replace(/\r/g, "").split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const trimmed = lines[i].trim();
    if (trimmed) return trimmed;
  }
  return "";
}

/** File icon SVG for attachment badges. */
export function FileIcon({ className = "" }: { className?: string }) {
  return <File className={className} size={12} strokeWidth={2} />;
}

/** Extract filename from path. */
export function fileName(path: string): string {
  // `/`-only split returned the entire C:\…\file.webm path (clipped the bubble).
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Render user message content, replacing file attachment markers with badges.
 * Marker format: \x00«file:/path/to/file» — uses NUL + guillemets to avoid
 * false positives with any user-typed text, markdown, or code.
 */
export interface ParsedVoice {
  transcript: string | null;
  note: string | null;
  fileUrl: string | null;
  fileName: string | null;
}

/** Parse the CodeTerm voice marker so the chat can show a voice card instead
 *  of the raw marker string. Returns null when the text isn't a voice line. */
export function parseVoiceMessage(content: string): ParsedVoice | null {
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

export function VoiceCard({ v }: { v: ParsedVoice }) {
  return (
    <div className="flex flex-col gap-1">
      <div
        className="flex flex-wrap items-center gap-1.5 text-[11px] min-w-0 max-w-full"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        <Mic size={13} strokeWidth={2} className="shrink-0" />
        <span className="opacity-80 font-medium shrink-0">Voice</span>
        {v.fileName && (
          <span className="opacity-40 font-mono min-w-0 break-all [overflow-wrap:anywhere]">{v.fileName}</span>
        )}
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

export interface ParsedPluginConfig {
  pluginId: string;
  name: string;
  body: string;
}

/** Parse the CodeTerm `plugin_config` marker so the chat can show a Configure
 *  card instead of the raw (core-wrapped) marker string. Returns null when the
 *  text isn't a plugin_config line. */
export function parsePluginConfigMessage(content: string): ParsedPluginConfig | null {
  const m = parseMarker(content.trim());
  if (!m || m.kind !== "plugin_config" || !m.attrs.plugin) return null;
  return {
    pluginId: m.attrs.plugin,
    name: m.attrs.name ?? m.attrs.plugin,
    body: m.payload,
  };
}

export function PluginConfigCard({ c }: { c: ParsedPluginConfig }) {
  return (
    <div className="flex flex-col gap-1">
      <div
        className="flex flex-wrap items-center gap-1.5 text-[11px] min-w-0 max-w-full"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        <Settings size={13} strokeWidth={2} className="shrink-0" />
        <span className="opacity-80 font-medium shrink-0">Configure {c.name}</span>
      </div>
      <pre className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{c.body}</pre>
    </div>
  );
}

export function renderUserContent(rawContent: string): React.ReactNode {
  const content = decodeCliMessageText(rawContent);
  const discussion = parseAttachedDiscussionDoc(content);
  if (discussion) return <AttachedDiscussionDocView doc={discussion} />;
  const voice = parseVoiceMessage(content);
  if (voice) return <VoiceCard v={voice} />;
  const pluginConfig = parsePluginConfigMessage(content);
  if (pluginConfig) return <PluginConfigCard c={pluginConfig} />;
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
  const attachRe = /(?:\x00\u00ABfile:([^\u00BB]+)\u00BB|\[attached:\s*([^\]]+)\])/g;
  const parts: React.ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = attachRe.exec(content)) !== null) {
    const before = content.slice(lastIndex, match.index).trim();
    if (before) {
      parts.push(<pre key={key++} className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{before}</pre>);
    }
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

  const rest = content.slice(lastIndex).trim();
  if (rest) {
    parts.push(<pre key={key++} className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{rest}</pre>);
  }

  if (parts.length === 0) {
    return <pre className={`${PRE_WRAP_CLASS} font-[inherit] m-0 select-text`}>{content}</pre>;
  }

  return <>{parts}</>;
}

/** Compact image indicator for standalone image messages. */
export function ImageBadge() {
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
export function CommandBody({ commands, body }: { commands: SlashCommand[]; body: string }) {
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
export function modelAvatar(model?: string): { glyph: string; color: string } {
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

export function ModelAvatar({ model }: { model?: string }) {
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
