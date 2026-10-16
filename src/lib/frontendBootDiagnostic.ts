import type { RequestFailure } from "./requestDiagnostics";
import { DEV_BUILD } from "./devBuild";

export type FrontendDiagnosticStage =
  | "bootstrap-injected" | "bootstrap-consumed" | "bootstrap-missing" | "bootstrap-restored" | "bootstrap-invalid"
  | "runtime-bound" | "module-entry" | "page-finished"
  | "frontend-initialize" | "frontend-initialized" | "frontend-initialize-failed"
  | "ready-request" | "ready-accepted" | "ready-rejected" | "ready-failed"
  | "script-error" | "promise-error" | "request-failed" | "chat-scroll";

declare global { interface Window { __CODETERM_SHELL_DIAGNOSTIC__?: (stage: string, failure?: { name?: string; reason?: string; httpStatus?: number; request?: RequestFailure; chat?: { scope: string; edge: string; at: number; state: object } }) => void } }

export function frontendBootDiagnostic(stage: FrontendDiagnosticStage, failure?: { name?: string; reason?: string; httpStatus?: number; request?: RequestFailure; chat?: { scope: string; edge: string; at: number; state: object } }): void {
  if (DEV_BUILD && typeof window !== "undefined") window.__CODETERM_SHELL_DIAGNOSTIC__?.(stage, failure);
}
