import React from "react";
import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import {
  Bot,
  Copy,
  FileText,
  Pencil,
  Search,
  Terminal,
} from "lucide-react";
import { recognize } from "./codeterm/recognize";
import { CodetermCard } from "./codeterm/CodetermCard";
import type { PaneNameResolver } from "./codeterm/templates";
import { CodeBlock, ChevronIcon, PRE_WRAP_CLASS, lineCount } from "./CodeBlock";
import {
  DiffView,
  extractCommand,
  extractEdits,
  extractFilePath,
  inferCodeLanguage,
  languageForEdits,
  languageFromPath,
  shortPath,
  TEXT_WRAP_CLASS,
} from "./MarkdownBody";
import { ToolIcon } from "./toolIcons";
import { InlineToolRow } from "./InlineToolRow";
import { ToolOutputContent } from "./ToolOutputContent";
import { isImageOnlyToolOutput, toolResultPreviewLine } from "./toolImageOutput";
import {
  contentSize,
  parsedToolInput,
  previewLine,
  resolveCuratedToolKind,
  shouldRenderInlineToolRow,
  textArg,
  toolDisplayLabel,
} from "./MessageBubble.helpers";

interface ToolCallBubbleProps {
  message: ChatMessage;
  expanded: boolean;
  onToggle: (next: boolean) => void;
  resolvePaneName?: PaneNameResolver;
  onOpenPlan?: (planId: string, taskId?: string) => void;
  fontScale?: number;
}

export function ToolCallBubble({
  message,
  expanded,
  onToggle,
  resolvePaneName,
  onOpenPlan,
  fontScale,
}: ToolCallBubbleProps) {
  const toolName = message.toolName || "";
  const filePath = extractFilePath(message);
  const command = extractCommand(message);
  const hasToolContent = message.toolInput?.content != null;
  const toolContent = hasToolContent ? String(message.toolInput!.content) : null;
  const edits = extractEdits(message);
  const toolStatus: "running" | "completed" | "failed" =
    message.toolError === true ? "failed" : (message.toolResult ? "completed" : "running");
  const statusGlyph = toolStatus === "running" ? "◜" : toolStatus === "failed" ? "✕" : "✓";
  const statusGlyphClass = toolStatus === "running" ? "animate-spin opacity-60" : (toolStatus === "completed" ? "opacity-60" : "opacity-100");
  const cardBorderStyle = toolStatus === "failed" ? "dashed" : "solid";
  const cardBorderColor = toolStatus === "failed"
    ? "color-mix(in srgb, var(--ct-border) 90%, var(--ct-fg) 10%)"
    : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)";
  const addedLines = edits.reduce((n, e) => n + (e.new ? e.new.split("\n").length : 0), 0);
  const removedLines = edits.reduce((n, e) => n + (e.old ? e.old.split("\n").length : 0), 0);

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

  const shellCommand = command;
  if (shellCommand) {
    const inv = recognize(shellCommand);
    if (inv) {
      return (
        <CodetermCard
          inv={inv}
          result={message.toolResult}
          expanded={expanded}
          onToggle={() => onToggle(!expanded)}
          resolvePaneName={resolvePaneName}
          onOpenPlan={onOpenPlan}
          fontScale={fontScale}
        />
      );
    }
  }

  const curatedKind = resolveCuratedToolKind(message.toolName);
  const showInlineRow = shouldRenderInlineToolRow(message, curatedKind, edits.length > 0);
  if (curatedKind) {
    const input = parsedToolInput(message);
    const Icon =
      curatedKind === "exec" ? Terminal :
      curatedKind === "read_file" ? FileText :
      curatedKind === "write_file" ? Pencil :
      curatedKind === "codeterm" ? Terminal :
      curatedKind === "mem_search" ? Search :
      Bot;
    const commandText =
      curatedKind === "exec"
        ? (textArg(input, ["cmd", "command"]) ?? extractCommand(message))
        : curatedKind === "codeterm" ? textArg(input, ["args", "command"]) :
      null;
    const pathText =
      curatedKind === "read_file" || curatedKind === "write_file"
        ? textArg(input, ["path", "file_path", "filePath"])
        : null;
    const queryText = curatedKind === "mem_search" ? textArg(input, ["query"]) : null;
    const providerText = curatedKind === "spawn_agent" ? textArg(input, ["provider"]) : null;
    const taskText = curatedKind === "spawn_agent" ? textArg(input, ["task"]) : null;
    const workspaceText = curatedKind === "spawn_agent" ? textArg(input, ["workspace"]) : null;
    const cwdText = curatedKind === "exec" ? textArg(input, ["cwd"]) : null;
    const contentText = curatedKind === "write_file" ? textArg(input, ["content"]) : null;
    const title =
      curatedKind === "exec" ? "shell" :
      curatedKind === "read_file" ? "read file" :
      curatedKind === "write_file" ? "write file" :
      curatedKind === "codeterm" ? "codeterm" :
      curatedKind === "mem_search" ? "mem search" :
      "spawn agent";
    const primary =
      commandText ?? pathText ?? queryText ?? taskText ?? message.toolArgs ?? message.content;
    const secondary =
      curatedKind === "exec" && cwdText ? `cwd ${cwdText}` :
      curatedKind === "write_file" && contentText ? `${contentSize(contentText)} | ${previewLine(contentText)}` :
      curatedKind === "spawn_agent"
        ? [providerText, workspaceText ? `workspace ${workspaceText}` : null].filter(Boolean).join(" | ")
        : null;
    if (showInlineRow) {
      const inlineGlyph = curatedKind === "read_file" ? "read" : (curatedKind === "mem_search" ? "search" : "generic");
      return (
        <InlineToolRow
          label={title}
          primaryArg={primary}
          resultText={message.toolResult}
          status={toolStatus === "completed" ? "ok" : (toolStatus === "failed" ? "failed" : "pending")}
          glyphKind={inlineGlyph}
          expanded={expanded}
          onToggle={() => onToggle(!expanded)}
          expandedContent={
            <>
              <pre className={`${PRE_WRAP_CLASS} m-0 font-mono`} style={{ color: "var(--ct-fg)" }}>
                {JSON.stringify(input, null, 2) || message.content}
              </pre>
              {message.toolResult && (
                <div className="mt-2">
                  <ToolOutputContent text={message.toolResult} maxHeight="16rem" />
                </div>
              )}
            </>
          }
        />
      );
    }

    return (
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto group">
        <div className="flex w-full min-w-0 flex-col">
          <button
            data-testid={`chatbackend-tool-${curatedKind}`}
            onClick={() => onToggle(!expanded)}
            className="flex w-full min-w-0 cursor-pointer items-center gap-1.5 text-left rounded-md border px-2.5 py-1.5 text-xs transition-colors duration-150 hover:brightness-110"
            style={{
              background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, black 4%)",
              borderColor: cardBorderColor,
              borderStyle: cardBorderStyle,
              color: "var(--ct-muted-fg)",
            }}
          >
            <span
              className={`w-4 text-center font-mono ${statusGlyphClass}`}
              style={{ color: "var(--ct-muted-fg)" }}
              title={toolStatus}
            >
              {statusGlyph}
            </span>
            <Icon
              size={13}
              strokeWidth={2}
              className="flex-shrink-0"
              style={{ color: "var(--ct-muted-fg)" }}
            />
            <span className="font-medium font-mono" style={{ color: "var(--ct-muted-fg)" }}>
              {title}
            </span>
            <span className="min-w-0 flex-1 truncate font-mono text-[11px]" style={{ color: "var(--ct-fg)" }}>
              {primary}
            </span>
            {secondary && (
              <span className="hidden min-w-0 truncate opacity-45 font-mono text-[11px] sm:inline">
                {secondary}
              </span>
            )}
            <ChevronIcon expanded={expanded} className="ml-1 opacity-0 transition-opacity group-hover:opacity-100" />
          </button>

          {message.toolResult && !expanded && !isImageOnlyToolOutput(message.toolResult) && (
            <button
              onClick={() => onToggle(true)}
              className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
              style={{ color: "var(--ct-muted-fg)" }}
              title="Show full output"
            >
              <span className="flex-shrink-0 opacity-40">{"→"}</span>
              <span className={`min-w-0 flex-1 ${TEXT_WRAP_CLASS} opacity-70`}>{toolResultPreviewLine(message.toolResult)}</span>
            </button>
          )}
          {message.toolResult && !expanded && isImageOnlyToolOutput(message.toolResult) && (
            <div className="mt-1 pl-2.5">
              <ToolOutputContent text={message.toolResult} />
            </div>
          )}

          {expanded && (
            <div
              className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2 text-xs"
              style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
            >
              {curatedKind === "write_file" && contentText ? (
                <CodeBlock text={contentText} language={languageFromPath(pathText)} maxHeight="24rem" />
              ) : (
                <pre className={`${PRE_WRAP_CLASS} m-0 font-mono`} style={{ color: "var(--ct-fg)" }}>
                  {JSON.stringify(input, null, 2) || message.content}
                </pre>
              )}
              {message.toolResult && (
                <div className="mt-2">
                  <div
                    className="mb-1 text-[10px] font-medium uppercase tracking-wide"
                    style={{ color: "var(--ct-muted-fg)" }}
                  >
                    Result
                  </div>
                  <button
                    type="button"
                    onClick={() => {
                      const payload = commandText
                        ? `$ ${commandText}\n\n${message.toolResult ?? ""}`
                        : (message.toolResult ?? "");
                      void navigator.clipboard.writeText(payload);
                    }}
                    className="mb-1 inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] opacity-0 transition-opacity group-hover:opacity-100"
                    style={{ color: "var(--ct-muted-fg)", border: "1px solid var(--ct-border)" }}
                  >
                    <Copy size={11} />
                    Copy
                  </button>
                  <ToolOutputContent text={message.toolResult} maxHeight="12rem" />
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  const detailText = message.toolArgs || message.content;
  const codeLanguage = edits.length > 0
    ? languageForEdits(filePath, edits)
    : inferCodeLanguage(filePath, toolContent ?? "");
  const resultText = message.toolResult;

  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto group">
      <div className="flex w-full min-w-0 flex-col">
        <button
          data-testid="chat-tool-call"
          onClick={() => onToggle(!expanded)}
          className="flex w-full min-w-0 cursor-pointer items-center gap-1.5 text-left rounded-md border px-2.5 py-1.5 text-xs transition-colors duration-150 hover:brightness-110"
          style={{
            background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, black 4%)",
            borderColor: cardBorderColor,
            borderStyle: cardBorderStyle,
            color: "var(--ct-muted-fg)",
          }}
        >
          <span
            className={`w-4 text-center font-mono ${statusGlyphClass}`}
            style={{ color: "var(--ct-muted-fg)" }}
            title={toolStatus}
          >
            {statusGlyph}
          </span>
          <span
            className="flex-shrink-0"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            <ToolIcon name={toolName} />
          </span>
          <span className="font-medium font-mono shrink-0" style={{ color: "var(--ct-muted-fg)" }}>
            {toolDisplayLabel(toolName)}
          </span>
          {summary && (
            <span className="min-w-0 flex-1 truncate font-mono text-[11px]" style={{ color: "var(--ct-fg)" }}>{summary}</span>
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
          <ChevronIcon expanded={expanded} className="ml-1 opacity-0 transition-opacity group-hover:opacity-100" />
        </button>

        {/* Compact inline tail — last line of output, always visible while
            collapsed. Click to reveal the full result under the command. */}
        {resultText && !expanded && !isImageOnlyToolOutput(resultText) && (
          <button
            onClick={() => onToggle(true)}
            className="mt-0.5 flex min-w-0 items-start gap-1 pl-2.5 text-left text-[11px] font-mono"
            style={{ color: "var(--ct-muted-fg)" }}
            title="Show full output"
          >
            <span className="flex-shrink-0 opacity-40">{"→"}</span>
            <span className={`min-w-0 flex-1 ${TEXT_WRAP_CLASS} opacity-70`}>{toolResultPreviewLine(resultText)}</span>
          </button>
        )}
        {resultText && !expanded && isImageOnlyToolOutput(resultText) && (
          <div className="mt-1 pl-2.5">
            <ToolOutputContent text={resultText} />
          </div>
        )}

        {expanded && (
          <div
            className="mt-1 w-full min-w-0 max-w-full rounded-md border border-[var(--ct-border)] p-2.5 text-xs"
            style={{ background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 4%)" }}
          >
            {toolContent ? (
              <CodeBlock text={toolContent} language={codeLanguage} maxHeight="24rem" />
            ) : edits.length > 0 ? (
              <DiffView edits={edits} language={codeLanguage} />
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
                <button
                  type="button"
                  onClick={() => {
                    const payload = command ? `$ ${command}\n\n${resultText ?? ""}` : (resultText ?? "");
                    void navigator.clipboard.writeText(payload);
                  }}
                  className="mb-1 inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] opacity-0 transition-opacity group-hover:opacity-100"
                  style={{ color: "var(--ct-muted-fg)", border: "1px solid var(--ct-border)" }}
                >
                  <Copy size={11} />
                  Copy
                </button>
                <ToolOutputContent text={resultText} maxHeight="12rem" />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
