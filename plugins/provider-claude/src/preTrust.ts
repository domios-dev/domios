import { ioValue, ioJson, type HostIoSteps } from "@codeterm/plugin-sdk";
import type { Host, PreTrustOutcome } from "@codeterm/plugin-sdk";

type TrustHost = Pick<Host, "envGet" | "homeDir" | "platform">;

export function claudeProjectKey(canonical: string, windows: boolean): string {
  if (!windows) return canonical;
  return canonical.replace(/\\/g, "/")
    .replace(/^\/\/\?\/UNC\//, "//")
    .replace(/^\/\/\?\//, "")
    .replace(/^[a-z]:/, (drive) => drive.toUpperCase());
}

export function* claudeTrustRecord(host: TrustHost, cwd: string, grant: boolean): HostIoSteps<PreTrustOutcome> {
  const root = host.envGet("CLAUDE_CONFIG_DIR") || host.homeDir();
  const canonical = (yield* ioValue<string>({ op: "canonicalPath", path: cwd }));
  if (!root) return { status: "skipped", reason: "configuration root unavailable" };
  if (!canonical) return { status: "skipped", reason: "launch cwd cannot be resolved" };
  const windows = host.platform() === "windows";
  const absoluteRoot = windows ? /^(?:[a-z]:[\\/]|[\\/]{2})/i.test(root) : root.startsWith("/");
  if (!absoluteRoot) return { status: "skipped", reason: "configuration root unavailable" };
  const key = claudeProjectKey(canonical, windows);
  const path = root.replace(/[\\/]$/, "") + "/.claude.json";
  try {
    return JSON.parse((yield* ioJson({ op: "jsonBooleanRecord", path: path, keys: ["projects", key, "hasTrustDialogAccepted"], grant: grant }))) as PreTrustOutcome;
  } catch {
    return { status: "failed", reason: "record capability failed" };
  }
}
