import type { FrontendDeviceDiagnostic } from "./frontendDiagnosticStamp";
import { DEV_BUILD } from "./devBuild";

const errors = ["Error", "ReferenceError", "TypeError", "SyntaxError", "SecurityError", "FrontendUpdateError"];
const reasons = ["invalid-client", "checkpoint-unavailable", "status-unavailable", "api-incompatible", "storage-unavailable", "boot-context-invalid", "unexpected", "native-capability-denied", "rejected-string", "rejected-value"];
type Failure = { name?: string; reason?: string; httpStatus?: number; filename?: string; lineno?: number; colno?: number; buildSha?: string; device?: FrontendDeviceDiagnostic; request?: unknown; chat?: unknown };
type Flags = { native: boolean; injected: boolean; mounted: boolean };

export function diagnosticBody(stage: string, failure: Failure | undefined, flags: Flags) {
  const source = String(failure?.filename ?? "").split("/").pop()!.split("?")[0].split("#")[0];
  return {
    stage, buildSha: failure?.buildSha, device: failure?.device,
    errorName: errors.includes(failure?.name ?? "") ? failure?.name : undefined,
    reason: reasons.includes(failure?.reason ?? "") ? failure?.reason : undefined,
    httpStatus: Number.isInteger(failure?.httpStatus) && failure!.httpStatus! >= 100 && failure!.httpStatus! <= 599 ? failure?.httpStatus : undefined,
    source: /^[a-zA-Z0-9_.-]{1,100}$/.test(source) ? source : undefined,
    request: failure?.request, chat: failure?.chat, line: failure?.lineno, column: failure?.colno,
    ...flags,
  };
}
type DiagnosticBody = ReturnType<typeof diagnosticBody>;

export function rejectionDiagnostic(value: unknown): Failure {
  if (typeof value === "string") {
    const denied = /^(?:[a-z][a-z0-9_-]*\.)?[a-z][a-z0-9_-]* (?:not allowed|explicitly denied)\b/.test(value);
    return { reason: denied ? "native-capability-denied" : "rejected-string" };
  }
  const failure = value as Failure | null;
  return { name: failure?.name, reason: failure?.reason ?? "rejected-value" };
}

export class DiagnosticQueue {
  private records: DiagnosticBody[] = [];
  private tokens = 10;
  private dropped = 0;
  constructor(private at: number) {}

  push(body: DiagnosticBody): void {
    try {
      const encoded = JSON.stringify(body);
      if (new TextEncoder().encode(encoded).length > 16 * 1024 - 64) { this.drop(); return; }
      if (this.records.length === 32) { this.records.shift(); this.drop(); }
      this.records.push(JSON.parse(encoded) as DiagnosticBody);
    } catch { this.drop(); }
  }

  drop(): void { this.dropped = Math.min(this.dropped + 1, 0xffffffff); }

  take(now: number): DiagnosticBody | undefined {
    this.tokens = Math.min(10, this.tokens + Math.max(0, now - this.at) / 2000);
    this.at = Math.max(now, this.at);
    if (this.tokens < 1 || !this.records.length) return undefined;
    this.tokens -= 1;
    const record = this.records.shift()!;
    if (this.dropped) {
      record.reason = `${record.reason ?? "unexpected"}; dropped=${this.dropped}`;
      this.dropped = 0;
    }
    return record;
  }
}

export function installWebDiagnosticPort(send: (body: DiagnosticBody) => Promise<unknown>): void {
  if (!DEV_BUILD || window.__CODETERM_SHELL_DIAGNOSTIC__) return;
  const queue = new DiagnosticQueue(performance.now());
  let sending = false;
  const flush = () => {
    if (sending) return;
    const body = queue.take(performance.now());
    if (!body) return;
    sending = true;
    void Promise.resolve().then(() => send(body)).catch(() => { queue.drop(); }).finally(() => { sending = false; flush(); });
  };
  const diagnostic = (stage: string, failure?: Failure) => {
    queue.push(diagnosticBody(stage, failure, {
      native: false, injected: !!window.__CODETERM_DAEMON__,
      mounted: document.documentElement?.dataset.codetermMounted === "true",
    }));
    queueMicrotask(flush);
  };
  window.__CODETERM_SHELL_DIAGNOSTIC__ = diagnostic;
  window.addEventListener("error", event => diagnostic("script-error", { name: event.error?.name, filename: event.filename, lineno: event.lineno, colno: event.colno }));
  window.addEventListener("unhandledrejection", event => diagnostic("promise-error", rejectionDiagnostic(event.reason)));
  window.addEventListener("online", flush);
  document.addEventListener("visibilitychange", flush);
}
