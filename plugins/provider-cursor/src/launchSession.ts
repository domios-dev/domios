import type { LaunchSessionDetection, LaunchSessionEvidence } from "@codeterm/plugin-sdk";

const MARKER = /^ct-launch-[a-f0-9]{32}$/;
const SESSION = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i;

function launchRecordPath(marker: string): string | null {
  const home = host.homeDir();
  if (!home || !MARKER.test(marker)) return null;
  return home + "/.cursor/projects/.codeterm-launches/" + marker + ".json";
}

export function cursorLaunchCommand(command: string, marker: unknown): string {
  if (typeof marker !== "string" || !MARKER.test(marker)) return command;
  const record = launchRecordPath(marker);
  if (!record) return "";
  const platform = host.platform();
  const quote = (value: string) => host.shell.quoteFor(value, platform);
  const resumed = command.replace("cursor-agent", platform === "windows"
    ? 'cursor-agent --resume "$ctCursorSession"'
    : 'cursor-agent --resume "$ct_cursor_session"');
  if (platform === "windows") {
    return "$ctCursorSession = (& cursor-agent create-chat | Out-String).Trim(); " +
      "if ($LASTEXITCODE -ne 0 -or $ctCursorSession -cnotmatch " + quote(SESSION.source) +
      ") { throw 'Cursor session creation failed' }; " +
      "[System.IO.Directory]::CreateDirectory(" + quote(record.substring(0, record.lastIndexOf("/"))) + ") | Out-Null; " +
      "[System.IO.File]::WriteAllText(" + quote(record) + ", (@{ launchMarker = " + quote(marker) +
      "; sessionId = $ctCursorSession } | ConvertTo-Json -Compress)); " + resumed;
  }
  return '(ct_cursor_session=$(cursor-agent create-chat) && ' +
    'case "$ct_cursor_session" in *[!0-9a-fA-F-]*|"") exit 1;; esac && ' +
    'case "$ct_cursor_session" in ????????-????-????-????-????????????) ;; *) exit 1;; esac && ' +
    "mkdir -p " + quote(record.substring(0, record.lastIndexOf("/"))) + " && " +
    "printf '%s\\n' \"{\\\"launchMarker\\\":\\\"" + marker +
    '\\\",\\\"sessionId\\\":\\\"$ct_cursor_session\\\"}" > ' + quote(record) +
    " && " + resumed + ")";
}

export function detectCursorLaunchSession(
  evidence: LaunchSessionEvidence,
  sessionExists: (cwd: string, sessionId: string) => boolean,
): LaunchSessionDetection | null {
  const marker = evidence?.launchMarker;
  if (typeof marker !== "string") return null;
  const path = launchRecordPath(marker);
  if (!path) return null;
  const record = host.fs.readJson<{ launchMarker?: unknown; sessionId?: unknown }>(path);
  if (!record || record.launchMarker !== marker || typeof record.sessionId !== "string" ||
      !SESSION.test(record.sessionId) || !sessionExists(evidence.cwd, record.sessionId)) return null;
  return { sessionId: record.sessionId, source: "launch_marker" };
}
