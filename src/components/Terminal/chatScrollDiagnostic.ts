import { DEV_BUILD } from "../../lib/devBuild";
import { frontendBootDiagnostic } from "../../lib/frontendBootDiagnostic";

export function chatScrollDiagnostic(scope: string, edge: string, state: object): void {
  if (DEV_BUILD) frontendBootDiagnostic("chat-scroll", { chat: { scope, edge, at: performance.now(), state } });
}
