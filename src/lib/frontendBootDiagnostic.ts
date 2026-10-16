import { streamClientId } from "./transportClient";
import type { RequestFailure } from "./requestDiagnostics";
import { buildHash } from "./buildStamp";
import { diagnosticBuildSha, type FrontendDeviceDiagnostic } from "./frontendDiagnosticStamp";
import { DEV_BUILD } from "./devBuild";

export type FrontendDiagnosticStage =
  | "bootstrap-injected" | "bootstrap-consumed" | "bootstrap-missing" | "bootstrap-restored" | "bootstrap-invalid"
  | "runtime-bound" | "module-entry" | "page-finished"
  | "frontend-initialize" | "frontend-initialized" | "frontend-initialize-failed"
  | "ready-request" | "ready-accepted" | "ready-rejected" | "ready-failed"
  | "script-error" | "promise-error" | "request-failed" | "chat-scroll";

declare global { interface Window { __CODETERM_SHELL_DIAGNOSTIC__?: (stage: string, failure?: { name?: string; reason?: string; httpStatus?: number; request?: RequestFailure; clientId?: string; buildSha?: string; device?: FrontendDeviceDiagnostic; chat?: { scope: string; edge: string; at: number; state: object } }) => void } }

export function frontendBootDiagnostic(stage: FrontendDiagnosticStage, failure?: { name?: string; reason?: string; httpStatus?: number; request?: RequestFailure; clientId?: string; buildSha?: string; device?: FrontendDeviceDiagnostic; chat?: { scope: string; edge: string; at: number; state: object } }): void {
  if (DEV_BUILD && typeof window !== "undefined") window.__CODETERM_SHELL_DIAGNOSTIC__?.(stage, { ...failure, clientId: streamClientId, buildSha: diagnosticBuildSha(buildHash()) });
}
