import { ioEncoded, type HostIoSteps } from "@codeterm/plugin-sdk";
import { effectiveReadPath } from "../../provider-shared/readHome";

type CredentialHost = Pick<import("@codeterm/plugin-sdk").Host, "io" | "expandHome" | "envGet" | "platform">;

export function* readClaudeOAuthToken(host: CredentialHost): HostIoSteps<string | null> {
  const inheritedHome = host.envGet("CLAUDE_CONFIG_DIR");
  const credentialsPath = effectiveReadPath(inheritedHome, host.expandHome("~/.claude/.credentials.json"), ".credentials.json", host.platform() === "windows" ? "\\" : "/");
  const fileCredentials = credentialsPath ? parseCredentials(yield* host.io.fs.readFile(credentialsPath)) : null;
  const credentials = (inheritedHome ? null : (yield* readMacOsCredentials(host))) ?? fileCredentials;
  const oauth = credentials?.claudeAiOauth;
  const token = oauth && typeof oauth === "object" ? (oauth as Record<string, unknown>).accessToken : null;
  return typeof token === "string" && token ? token : null;
}

function* readMacOsCredentials(host: CredentialHost): HostIoSteps<Record<string, unknown> | null> {
  if (host.platform() !== "macos") return null;
  const user = host.envGet("USER");
  const args = ["find-generic-password"];
  if (user) args.push("-a", user);
  args.push("-w", "-s", "Claude Code-credentials");
  const result = parseCredentials(yield* ioEncoded(host.io.exec({ bin: "security", args, timeoutMs: 5000 })));
  if (result?.code !== 0) return null;
  return parseCredentials(result.stdout);
}

function parseCredentials(raw: unknown): Record<string, unknown> | null {
  if (typeof raw !== "string" || !raw.trim()) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : null;
  } catch {
    return null;
  }
}
