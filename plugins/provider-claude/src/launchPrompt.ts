import QUESTION_RULE from "./launchPromptRule.json";

export function writeClaudeLaunchPrompt(task: string): string | null {
  const suffix = ".ct-task-" + Math.random().toString(16).slice(2, 10);
  const fragment = task + "\n\n" + QUESTION_RULE;
  const body = host.platform() === "windows" ? "\uFEFF" + fragment : fragment;
  return host.fs.writeTempFile(body, suffix);
}
