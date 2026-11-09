import { type HostIoSteps } from "@codeterm/plugin-sdk";
import QUESTION_RULE from "./launchPromptRule.json";

export function* writeClaudeLaunchPrompt(task: string): HostIoSteps<string | null> {
  const suffix = ".ct-task-" + Math.random().toString(16).slice(2, 10);
  const fragment = task + "\n\n" + QUESTION_RULE;
  const body = host.platform() === "windows" ? "\uFEFF" + fragment : fragment;
  return (yield* host.io.fs.writeTempFile(body, suffix));
}
