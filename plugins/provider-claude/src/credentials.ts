import { ioEncoded, type HostIoSteps } from "@codeterm/plugin-sdk";
import { effectiveReadPath } from "../../provider-shared/readHome";

type CredentialHost = Pick<import("@codeterm/plugin-sdk").Host, "io" | "expandHome" | "envGet" | "platform" | "sha256">;

export function* readClaudeOAuthToken(host: CredentialHost): HostIoSteps<string | null> {
  const configuredHome = host.envGet("CLAUDE_SECURESTORAGE_CONFIG_DIR") || host.envGet("CLAUDE_CONFIG_DIR");
  const credentialHome = configuredHome?.startsWith("~/") || configuredHome === "~"
    ? host.expandHome(configuredHome)
    : configuredHome;
  if (configuredHome && !credentialHome) return null;
  const keychainToken = oauthToken(yield* readMacOsCredentials(host, credentialHome));
  if (keychainToken) return keychainToken;

  const credentialsPath = effectiveReadPath(credentialHome, host.expandHome("~/.claude/.credentials.json"), ".credentials.json", host.platform() === "windows" ? "\\" : "/");
  return credentialsPath ? oauthToken(parseCredentials(yield* host.io.fs.readFile(credentialsPath))) : null;
}

function oauthToken(credentials: Record<string, unknown> | null): string | null {
  const oauth = credentials?.claudeAiOauth;
  const token = oauth && typeof oauth === "object" ? (oauth as Record<string, unknown>).accessToken : null;
  return typeof token === "string" && token ? token : null;
}

function* readMacOsCredentials(host: CredentialHost, home: string | null): HostIoSteps<Record<string, unknown> | null> {
  if (host.platform() !== "macos") return null;
  const suffix = home ? host.sha256?.(home.replace(/\/+$/, "")).slice(0, 8) : null;
  if (home && !suffix) return null;
  const service = suffix ? `Claude Code-credentials-${suffix}` : "Claude Code-credentials";
  const user = host.envGet("USER");
  const args = ["find-generic-password"];
  if (user) args.push("-a", user);
  args.push("-w", "-s", service);
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
