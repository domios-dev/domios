/**
 * Provider plugin contract — implemented by each `plugins/provider-<id>/`
 * workspace package and consumed by the chat host (ChatView).
 *
 * Plugins are pure transforms + a React component. They hold no
 * stateful classes, no host references, no hidden caches. The host
 * (codeterm) owns the events list and current status; the plugin maps
 * lines → events, events → status, and renders. This is what makes
 * hot-reload trivial — replacing the plugin module just swaps function
 * refs and re-renders.
 *
 * `Renderer`'s fallback contract: when the plugin has no useful events
 * yet (fresh pane, no JSONL seen), it should mount `raw()` so the user
 * still sees terminal output. Never render an empty box.
 */
import type { ReactNode } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

/** Coarse pane status the host needs to know about for badges /
 *  notifications. Plugins map their own events to this. */
export type PaneStatus =
  | { kind: "idle" }
  | { kind: "agent_working" }
  | { kind: "agent_waiting_input"; question?: string }
  | { kind: "agent_done" }
  | { kind: "agent_error"; message?: string }
  | { kind: "exited"; code: number };

/** One semantic event extracted from a provider's session stream
 *  (JSONL line, PTY chunk, hook callback, ...). `kind` is plugin-
 *  defined ("user_message" / "tool_call" / "thinking" / etc); the host
 *  treats it as opaque except for delegating renderer + status mapping
 *  back to the same plugin. */
export interface ProviderEvent {
  kind: string;
  ts?: number;
  payload?: unknown;
}

/** Props the host passes to the plugin's Renderer. */
export interface RendererProps {
  /** Parsed events from the plugin's own parseLine; chronological. */
  events: ProviderEvent[];
  /** Mount the raw PTY view (xterm passthrough). Use as a fallback or
   *  as a "raw view" toggle inside the plugin's UI. */
  raw: () => ReactNode;
  /** Send text into the pane via transport. Returns when the host
   *  acknowledges the write (not when the provider sees it). */
  send: (text: string) => Promise<void>;
  /** Latest pane status the host knows about. */
  status: PaneStatus;
  /** Pane id — useful for plugin-side per-pane caches, never for I/O. */
  paneId: string;
}

/** Plugin-contributed button/menu item shown in pane chrome. */
export interface ProviderAction {
  id: string;
  label: string;
  /** Run the action. Returns text to forward via `send()`, or `null`
   *  to cancel. Pure — host calls send() afterwards. */
  run: (ctx: { events: ProviderEvent[] }) => string | null;
}

/** Heuristic sample passed to plugin.detect() when the host is trying
 *  to figure out which provider lives in a freshly-opened pane. */
export interface DetectionSample {
  /** Last ~2 KB of PTY output (best-effort window). */
  bytes: string;
  /** Pane title at time of detection, if any. */
  title?: string;
}

/** How a provider expects file attachments to be delivered. */
export interface ImageSendStrategy {
  /** `clipboard-paste` = copy to OS clipboard then send a paste keystroke.
   *  `path` = inject the file path as text in the chat input. */
  method: "clipboard-paste" | "path";
  /** For clipboard-paste: keystroke to send (e.g. `\x16` for Ctrl+V). */
  pasteKeystroke?: string;
}

/** The contract every provider plugin satisfies. The shape is stable
 *  within a major host API version. */
export interface ProviderPlugin {
  meta: {
    /** Stable id; must match `provider_id` in plugin.json AND the Rust
     *  `AgentProvider::id()`. Host uses this to look up the plugin. */
    id: string;
    displayName: string;
  };

  /** Cheap structural fingerprint check. Called repeatedly during
   *  pane warm-up. No I/O. */
  detect(sample: DetectionSample): boolean;

  /** Parse one JSONL session line into structured events. May emit
   *  zero or many. Called per-line as the host streams new content
   *  from the provider's session file. */
  parseLine(line: string): ProviderEvent[];

  /** Parse a whole JSONL session blob into chat messages for the chat
   *  view. JSONL providers implement this (delegating to their session
   *  parser); structured/DB-backed providers omit it (the host renders
   *  Rust-parsed messages instead). */
  parseSession?: (raw: string) => NormalizedChatMessage[];

  /** Map a single event to a coarse pane status. `null` means "no
   *  status change — keep whatever the host had before". */
  statusOf(event: ProviderEvent): PaneStatus | null;

  /** React component rendering the chat view. See `RendererProps`. */
  Renderer: (props: RendererProps) => ReactNode;

  /** Pane-chrome actions (buttons, menu items). Empty array is fine. */
  actions: ProviderAction[];

  /** TUI noise patterns — text matching any pattern is dropped from
   *  the fallback parser / activity log. Optional. */
  noisePatterns?: RegExp[];

  /** Title pattern for `detect_from_title` style matching. The host
   *  uses this on tab rename events when no PTY sample is available
   *  yet. Optional — plugins can rely on `detect(sample)` instead. */
  titlePattern?: RegExp;

  /** Strategy for sending file attachments. Falls back to `path` mode
   *  when omitted (the plugin's CLI just receives the file path as
   *  text). Claude wants `clipboard-paste` because its TUI listens
   *  for OS clipboard input on Ctrl+V. */
  imageSend?: ImageSendStrategy;
}
