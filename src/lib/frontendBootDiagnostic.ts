import type { RequestFailure } from "./requestDiagnostics";
import { DEV_BUILD } from "./devBuild";

declare global { interface Window { __CODETERM_SHELL_DIAGNOSTIC__?: (stage: string, failure?: { name?: string; reason?: string; httpStatus?: number; request?: RequestFailure }) => void } }

export function frontendBootDiagnostic(stage: string, failure?: { name?: string; reason?: string; httpStatus?: number; request?: RequestFailure }): void {
  if (DEV_BUILD && typeof window !== "undefined") window.__CODETERM_SHELL_DIAGNOSTIC__?.(stage, failure);
}
