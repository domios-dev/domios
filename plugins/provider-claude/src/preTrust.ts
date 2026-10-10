import type { Host, PreTrustOutcome } from "@codeterm/plugin-sdk";

type TrustHost = Pick<Host, "envGet" | "homeDir" | "platform" | "canonicalPath" | "jsonBooleanRecord">;

export function claudeProjectKey(canonical: string, windows: boolean): string {
  if (!windows) return canonical;
  return canonical.replace(/\\/g, "/")
    .replace(/^\/\/\?\/UNC\//, "//")
    .replace(/^\/\/\?\//, "")
    .replace(/^[a-z]:/, (drive) => drive.toUpperCase());
}

export function claudeTrustRecord(host: TrustHost, cwd: string, grant: boolean): PreTrustOutcome {
  const root = host.envGet("CLAUDE_CONFIG_DIR") || host.homeDir();
  const canonical = host.canonicalPath(cwd);
  if (!root) return { status: "skipped", reason: "configuration root unavailable" };
  if (!canonical) return { status: "skipped", reason: "launch cwd cannot be resolved" };
  const windows = host.platform() === "windows";
  const absoluteRoot = windows ? /^(?:[a-z]:[\\/]|[\\/]{2})/i.test(root) : root.startsWith("/");
  if (!absoluteRoot) return { status: "skipped", reason: "configuration root unavailable" };
  const key = claudeProjectKey(canonical, windows);
  const path = root.replace(/[\\/]$/, "") + "/.claude.json";
  try {
    return JSON.parse(host.jsonBooleanRecord(path, JSON.stringify(["projects", key, "hasTrustDialogAccepted"]), grant)) as PreTrustOutcome;
  } catch {
    return { status: "failed", reason: "record capability failed" };
  }
}
