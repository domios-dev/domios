import React, { useCallback, useState } from "react";
import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { CheckCircle2, Copy, Loader, RotateCcw, Terminal, XCircle } from "lucide-react";
import { shortPath } from "./MarkdownBody";

interface Props {
  message: ChatMessage;
  /** Re-run the command in the pane's PTY (VS Code command-decoration rerun). */
  onRerun?: (command: string) => void;
  /** Surface the command's terminal output (jump-to-command). */
  onJump?: (message: ChatMessage) => void;
}

const STATUS = {
  running: { Icon: Loader, color: "var(--ct-muted-fg, #8b949e)", spin: true },
  ok: { Icon: CheckCircle2, color: "var(--ct-green, #3fb950)", spin: false },
  failed: { Icon: XCircle, color: "var(--ct-red, #f85149)", spin: false },
} as const;

/** A first-class shell command chat item with VS Code-style decorations:
 *  status glyph + exit code, cwd, and rerun / jump / copy affordances. Renders
 *  from the deterministic OSC 133/633 lifecycle — never assumes output-start, so
 *  it stays valid in A/D-only degraded mode (boundary + exit + cwd only). */
export const ShellCommandItem = React.memo(function ShellCommandItem({ message, onRerun, onJump }: Props) {
  const [copied, setCopied] = useState(false);
  const command = message.shellCommand ?? message.content ?? "";
  const status = message.shellStatus ?? "ok";
  const { Icon, color, spin } = STATUS[status];

  const copy = useCallback(() => {
    if (!command) return;
    void navigator.clipboard?.writeText(command);
    setCopied(true);
    setTimeout(() => setCopied(false), 1_200);
  }, [command]);

  const showExit = status === "failed" && message.shellExitCode != null;

  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto group" data-testid="chat-shell-command" data-shell-status={status}>
      <div
        className="flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs font-mono"
        style={{
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, black 4%)",
          borderColor: status === "failed"
            ? "color-mix(in srgb, var(--ct-red, #f85149) 35%, transparent)"
            : "var(--ct-border, #30363d)",
          color: "var(--ct-fg, #c9d1d9)",
        }}
      >
        <Icon
          size={13}
          strokeWidth={2}
          className={spin ? "animate-spin" : undefined}
          style={{ stroke: color, flexShrink: 0 }}
          aria-label={status}
          data-testid="chat-shell-status-glyph"
        />
        <Terminal size={12} strokeWidth={2} style={{ stroke: "var(--ct-muted-fg)", flexShrink: 0 }} aria-hidden />
        <span className="min-w-0 flex-1 truncate" title={command} data-testid="chat-shell-command-text">
          {command || <span className="opacity-50">(no command captured)</span>}
        </span>
        {showExit && (
          <span
            className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold"
            data-testid="chat-shell-exit"
            style={{
              background: "color-mix(in srgb, var(--ct-red, #f85149) 18%, transparent)",
              color: "var(--ct-red, #f85149)",
            }}
          >
            exit {message.shellExitCode}
          </span>
        )}
        {message.shellCwd && (
          <span className="shrink-0 truncate max-w-[40%] opacity-60" title={message.shellCwd} data-testid="chat-shell-cwd">
            {shortPath(message.shellCwd)}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
          <button
            type="button"
            onClick={copy}
            disabled={!command}
            title={copied ? "Copied" : "Copy command"}
            data-testid="chat-shell-copy"
            className="rounded p-1 hover:bg-[var(--ct-bg-hover,rgba(255,255,255,0.06))] disabled:opacity-30"
          >
            {copied
              ? <CheckCircle2 size={12} strokeWidth={2} style={{ stroke: "var(--ct-green, #3fb950)" }} />
              : <Copy size={12} strokeWidth={2} style={{ stroke: "var(--ct-muted-fg)" }} />}
          </button>
          {onRerun && command && (
            <button
              type="button"
              onClick={() => onRerun(command)}
              title="Re-run command"
              data-testid="chat-shell-rerun"
              className="rounded p-1 hover:bg-[var(--ct-bg-hover,rgba(255,255,255,0.06))]"
            >
              <RotateCcw size={12} strokeWidth={2} style={{ stroke: "var(--ct-muted-fg)" }} />
            </button>
          )}
          {onJump && (
            <button
              type="button"
              onClick={() => onJump(message)}
              title="Jump to command output"
              data-testid="chat-shell-jump"
              className="rounded p-1 hover:bg-[var(--ct-bg-hover,rgba(255,255,255,0.06))]"
            >
              <Terminal size={12} strokeWidth={2} style={{ stroke: "var(--ct-muted-fg)" }} />
            </button>
          )}
        </span>
      </div>
    </div>
  );
});
