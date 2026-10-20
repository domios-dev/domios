import type { HostOwnedCapabilities, LaunchParams, ResumeParams } from "@codeterm/plugin-sdk";
import { quoteNativeArgument } from "../../provider-shared/nativeArgument";
import { statuslineLaunchArgs } from "./context";
import { writeClaudeLaunchPrompt } from "./launchPrompt";

const FILE_TASK_PROMPT =
  "Execute the CodeTerm task provided in the appended system prompt. Follow its reporting instructions exactly.";
const IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";

export function starterPrompt(prompt: LaunchParams["starterPrompt"], hostText?: string): string | undefined {
  if (hostText) return hostText;
  if (prompt === "no_starter") return undefined;
  if (prompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (prompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return FILE_TASK_PROMPT;
}

export function hostOwnedLaunchArgs(hostOwned?: HostOwnedCapabilities): string[] {
  const tools = [
    ...(hostOwned?.questions ? ["AskUserQuestion"] : []),
    ...(hostOwned?.todos ? ["TodoWrite"] : []),
  ];
  return tools.length ? ["--disallowedTools", tools.join(",")] : [];
}

function hasResume(input: LaunchParams | { resume: ResumeParams }): input is { resume: ResumeParams } {
  const resume = input.resume;
  return typeof resume === "object" && resume !== null && "sessionId" in resume && typeof resume.sessionId === "string";
}

export function buildClaudeLaunchSpec(input: LaunchParams | { resume: ResumeParams }) {
  if ("resume" in input && !hasResume(input)) return null;
  const params: LaunchParams = hasResume(input) ? {
    args: ["--resume", input.resume.sessionId, ...(input.resume.args || [])],
    task: input.resume.systemPrompt ?? undefined,
    skipPermissions: input.resume.skipPermissions,
    hostOwned: input.resume.hostOwned,
    starterPrompt: "no_starter",
  } : input;
  const args: string[] = [];
  if (params.skipPermissions) args.push("--dangerously-skip-permissions");
  if (params.agentName) args.push("--agent", params.agentName);
  if (params.sessionId) args.push("--session-id", params.sessionId);
  const extra = statuslineLaunchArgs(params.args || []);
  if (!extra) return null;
  args.push(...extra);
  args.push(...hostOwnedLaunchArgs(params.hostOwned));
  const path = writeClaudeLaunchPrompt(params.task || "");
  if (!path) return null;
  args.push("--append-system-prompt-file", path);
  const prompt = params.task ? starterPrompt(params.starterPrompt, params.starterPromptText) : undefined;
  if (prompt) {
    const framed = params.starterPromptPrefix ? params.starterPromptPrefix + prompt + "</domios>" : prompt;
    args.push("--", framed);
  }
  return { program: "claude", args, envSet: {}, envUnset: ["CLAUDECODE"] };
}

export function buildClaudeLaunchCommand(params: LaunchParams): string {
  const spec = buildClaudeLaunchSpec(params);
  if (!spec) return "";
  const unset = host.platform() === "windows"
    ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; "
      + "$OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = "
      + "[System.Text.Encoding]::UTF8; "
      + "$PSNativeCommandArgumentPassing = 'Standard';"
    : "unset CLAUDECODE &&";
  const parts = [unset, spec.program];
  let previous = "";
  for (const arg of spec.args) {
    if (previous === "--") parts.push(quoteNativeArgument(arg, host.platform(), host.shell.quoteFor));
    else if (["--agent", "--session-id", "--append-system-prompt-file"].includes(previous)) {
      parts.push(host.shell.quoteFor(arg, host.platform()));
    } else {
      parts.push(/^[A-Za-z0-9_\-.,:=@/+%]+$/.test(arg) ? arg : host.shell.quoteFor(arg, host.platform()));
    }
    previous = arg;
  }
  return parts.join(" ");
}
