import { commandShape } from "./commandShape";
import type { CodetermInvocation } from "./invocation";

export type { CodetermInvocation } from "./invocation";

/** The CodeTerm invocation a command is, when it is exactly one CodeTerm action and nothing else. */
export function recognize(command: string, argv?: readonly string[]): CodetermInvocation | null {
  const shape = commandShape(command, argv);
  return shape.kind === "codeterm" ? shape.invocation : null;
}
