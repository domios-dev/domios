import { DEV_BUILD } from "./devBuild";

declare global { interface Window { __CODETERM_SHELL_DIAGNOSTIC__?: (stage: string) => void } }

export function frontendBootDiagnostic(stage: string): void {
  if (DEV_BUILD && typeof window !== "undefined") window.__CODETERM_SHELL_DIAGNOSTIC__?.(stage);
}
