import fs from "node:fs";
import path from "node:path";
import { claudeFolderTrustHighlightedOptionIndex, claudeFolderTrustPrompt } from "../src/folderTrust";

interface TrustPlugin {
  directoryTrustPromptResponse(screen: string): unknown;
  parsePrompt(screen: string): unknown;
  parseDirectoryTrustPrompt(screen: string): unknown;
}

export function runFolderTrustRegionTests(
  hostPlugin: TrustPlugin,
  dir: string,
  eq: (actual: unknown, expected: unknown, name: string) => void,
  assert: (condition: boolean, name: string) => void,
): void {
  const quoted = fs.readFileSync(path.join(dir, "claude-transcript-quotes-trust-dialog.txt"), "utf8");
  eq(hostPlugin.directoryTrustPromptResponse(quoted), null, "transcript quote has no live trust response");
  eq(hostPlugin.parsePrompt(quoted), null, "transcript quote has no live prompt");
  eq(hostPlugin.parseDirectoryTrustPrompt(quoted), null, "transcript quote has no live directory trust prompt");
  eq(claudeFolderTrustPrompt(quoted), null, "transcript quote has no trust prompt");
  eq(claudeFolderTrustHighlightedOptionIndex(quoted), null, "transcript quote has no trust focus");
  const trailingBlanks = fs.readFileSync(path.join(dir, "claude-2.1.296-trailing-blanks.txt"), "utf8");
  eq(hostPlugin.directoryTrustPromptResponse(trailingBlanks), ["ag=="], "blank tail preserves the live trust response");
  assert(Boolean(claudeFolderTrustPrompt(trailingBlanks)), "blank tail preserves the live trust prompt");
  eq(claudeFolderTrustHighlightedOptionIndex(trailingBlanks), 0, "blank tail preserves the live trust focus");
}
