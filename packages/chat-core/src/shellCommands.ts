import type { NormalizedChatMessage,ShellCommandStatus } from "./types";

/** Mirror of the Rust `ShellCommand` (serde camelCase) — one finished command. */
export type ShellCommandSnapshotItem = import("./gen/ShellCommand").ShellCommand;

/** Mirror of the Rust `ShellCapabilitySnapshot` returned by `shell_command_snapshot`. */
export type ShellCommandSnapshot = import("./gen/ShellCapabilitySnapshot").ShellCapabilitySnapshot;

/** Mirror of the Rust `CommandLifecycle` (serde tag "kind", camelCase) carried by
 *  the `shell-command-changed` event payload's `event` field. */
export type CommandLifecycle = import("./gen/CommandLifecycle").CommandLifecycle;

const TYPE: NormalizedChatMessage["type"] = "shell_command";

/** exit 0 (or unknown) → ok; any non-zero → failed. Running has no exit yet. */
function statusFor(exitCode: number | undefined): ShellCommandStatus {
  if (exitCode == null) return "ok";
  return exitCode === 0 ? "ok" : "failed";
}

function isShell(m: NormalizedChatMessage): boolean {
  return m.type === TYPE;
}

/** Build a finished `shell_command` chat item from a snapshot entry. */
export function shellCommandMessage(
  item: ShellCommandSnapshotItem,
  id: string,
  timestampMs: number,
): NormalizedChatMessage {
  return {
    id,
    type: TYPE,
    content: item.commandLine ?? "",
    timestamp: String(timestampMs),
    shellCommand: item.commandLine,
    shellStatus: statusFor(item.exitCode),
    shellExitCode: item.exitCode,
    shellCwd: item.cwd,
    shellHasOutputStart: item.hasOutputStart,
  };
}

/** Hydrate the snapshot (all entries are finished commands) into chat items.
 *  Timestamps increase by index from `baseTimestampMs` so they sort in shell
 *  order and ahead of any live items stamped with the current clock. */
export function shellSnapshotToMessages(
  snapshot: ShellCommandSnapshot,
  idPrefix: string,
  baseTimestampMs: number,
): NormalizedChatMessage[] {
  return snapshot.commands.map((item, i) =>
    shellCommandMessage(item, `${idPrefix}-${i}`, baseTimestampMs + i),
  );
}

/** Fold one live `CommandLifecycle` event into the shell-command item list.
 *  Returns the same array reference when nothing changes (cwd-only events).
 *
 *  Correlation: a shell runs one command at a time, so `finished` updates the
 *  most recent still-`running` item in place. With no running item (A/D-only
 *  degraded mode emits no `started`/`C`), it appends a finished item directly —
 *  boundary + exit + cwd stay valid without assuming output-start. */
export function applyShellLifecycle(
  messages: NormalizedChatMessage[],
  event: CommandLifecycle,
  next: { id: string; timestampMs: number },
): NormalizedChatMessage[] {
  if (event.kind === "cwdChanged") return messages;

  if (event.kind === "started") {
    return [
      ...messages,
      {
        id: next.id,
        type: TYPE,
        content: event.command_line ?? "",
        timestamp: String(next.timestampMs),
        shellCommand: event.command_line,
        shellStatus: "running",
        shellCwd: event.cwd,
        shellHasOutputStart: true,
      },
    ];
  }

  // finished — update the last running item, else append a fresh finished one.
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (isShell(m) && m.shellStatus === "running") {
      const updated: NormalizedChatMessage = {
        ...m,
        content: event.command_line ?? m.content,
        shellCommand: event.command_line ?? m.shellCommand,
        shellStatus: statusFor(event.exit_code),
        shellExitCode: event.exit_code,
        shellCwd: event.cwd ?? m.shellCwd,
      };
      const copy = messages.slice();
      copy[i] = updated;
      return copy;
    }
  }

  return [
    ...messages,
    shellCommandMessage(
      {
        commandLine: event.command_line,
        exitCode: event.exit_code,
        cwd: event.cwd,
        hasOutputStart: false,
      },
      next.id,
      next.timestampMs,
    ),
  ];
}
