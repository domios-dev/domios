import type { HostOwnedCapabilities, LaunchParams, ResumeParams } from "@codeterm/plugin-sdk";
import { withLaunchMarker } from "./launchSession";
import { extractSessionUuid } from "./sessions";

export const CODEX_LAUNCH_CONFIG = ["-c", "check_for_update_on_startup=false", "-c", "project_doc_max_bytes=65536"];
export const POSIX_ARGV_BUDGET = 256 * 1024;

export function hostOwnedCodexLaunchArgs(hostOwned?: HostOwnedCapabilities): string[] {
  // Codex only gates request_user_input in Default mode; Plan mode remains enabled.
  return hostOwned?.questions ? ["--disable", "default_mode_request_user_input"] : [];
}

export function utf8Bytes(value: string): number {
  let bytes = 0;
  for (const character of value) {
    const code = character.codePointAt(0)!;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return bytes;
}

function supportsDirectResume(): boolean {
  try {
    const minimum = "0.159.0";
    const observed = JSON.parse(host.providerVersionJson());
    if (observed?.status !== "detected" || typeof minimum !== "string") return false;
    const parse = (value: string) => /^(\d+)\.(\d+)\.(\d+)$/.exec(value)?.slice(1).map(Number);
    const version = parse(String(observed.version));
    const bound = parse(minimum);
    if (!version || !bound) return false;
    for (let index = 0; index < 3; index++) {
      if (version[index] !== bound[index]) return version[index] > bound[index];
    }
    return true;
  } catch {
    return false;
  }
}

export function buildCodexLaunchSpec(input: LaunchParams | { resume: ResumeParams }, starter: (params: LaunchParams) => string | undefined) {
  const platform = host.platform();
  if (platform === "windows") return null;
  const hostOwned = "resume" in input
    ? (input.resume as ResumeParams | undefined)?.hostOwned
    : input.hostOwned;
  const args = [...CODEX_LAUNCH_CONFIG];
  if ("resume" in input) {
    const resume = input.resume as ResumeParams | undefined;
    if (!resume || typeof resume.sessionId !== "string" || !supportsDirectResume()) return null;
    args.push("resume");
    if (resume.skipPermissions) args.push("--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust");
    args.push(extractSessionUuid(resume.sessionId) || resume.sessionId, "--no-daemon", ...(resume.args || []));
    args.push(...hostOwnedCodexLaunchArgs(hostOwned));
  } else {
    if (input.skipPermissions) args.push("--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust");
    args.push(...(input.args || []));
    args.push(...hostOwnedCodexLaunchArgs(hostOwned));
    if (input.task?.trim()) {
      const instructions = withLaunchMarker(input.task, input.launchMarker).replace(/\n+$/, "");
      args.push("-c", "developer_instructions=" + instructions);
      const prompt = starter(input);
      if (prompt) args.push("--", prompt);
    }
  }
  const values = ["codex", ...args];
  if (values.some(value => value.includes("\0"))) return null;
  if (values.reduce((bytes, value) => bytes + utf8Bytes(value) + 1, 0) > POSIX_ARGV_BUDGET) return null;
  // Linux also bounds each string, including its terminating NUL, to 32 pages.
  if (platform === "linux" && values.some(value => utf8Bytes(value) + 1 > 128 * 1024)) return null;
  let exclusions: string[];
  try {
    exclusions = JSON.parse(host.manifestJson() || "null")?.capabilities?.agentRuntime?.inheritedEnvironmentExclusions;
  } catch {
    return null;
  }
  if (!Array.isArray(exclusions) || exclusions.some(value => typeof value !== "string")) return null;
  return { program: "codex", args, envSet: {}, envUnset: [...exclusions] };
}

export function codexCommandFromSpec(spec: NonNullable<ReturnType<typeof buildCodexLaunchSpec>>): string {
  const quote = (value: string) => host.shell.quoteFor(value, host.platform());
  const unset = spec.envUnset.length ? "unset " + spec.envUnset.map(quote).join(" ") + " && " : "";
  return unset + [spec.program, ...spec.args].map(value => /^[A-Za-z0-9_\-.,:=@/+%]+$/.test(value) ? value : quote(value)).join(" ");
}
