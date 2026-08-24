import type { CodetermInvocation } from "./recognize";

/** What a routed message is on the wire: one body, one recipient tab. Built
 *  here so a caller that already holds both renders through the existing sent
 *  bubble rather than authoring a second sent-message shape. */
export function sentInvocation(tabId: string, body: string): CodetermInvocation {
  return {
    binary: "codeterm",
    path: ["send"],
    flags: { tab: tabId },
    positionals: [body],
    raw: `codeterm send --tab ${tabId}`,
  };
}
