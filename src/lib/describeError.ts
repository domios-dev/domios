export type ErrorAction = "choose-folder" | "retry" | "sign-in" | "check-access" | "choose-smaller-file" | "check-executable";
export interface ErrorDescription {
  title: string;
  cause: string;
  actions: ErrorAction[];
  raw: string;
}
export interface ErrorContext { operation?: string; cwd?: string | null }

const runtimeFailures: readonly (readonly [RegExp, string, string])[] = [
  [/\bSizeLimit\b|runtime(?: hook state)? size limit/i, "The agent runtime exceeded its size limit", "The runtime could not accept more data. Review the details and start a new session if needed."],
  [/^(?:SpawnFailed\b|Runtime could not start:)/, "The agent runtime could not start", "The runtime program failed to start. Check its installation before trying again."],
  [/^(?:ProcessExited\b|Runtime (?:exited|stopped)\b)/, "The agent runtime stopped", "The runtime process ended. Retry to reconnect to the agent."],
  [/^(?:PipeEof\b|Runtime connection closed)/, "The connection to the agent runtime closed", "The runtime connection ended. Retry to reconnect."],
  [/^(?:PipeFailure\b|Runtime \S+ connection failed:)/, "The connection to the agent runtime failed", "Domios could not communicate with the runtime. Retry to reconnect."],
  [/^(?:InvalidFrame\b|Runtime sent an invalid response:)/, "The agent runtime sent an invalid response", "Domios could not read the runtime response. Review the details and retry."],
  [/^(?:HookFailed\b|Provider operation failed:)/, "The provider plugin failed while handling a response", "The plugin could not finish this operation. Review its details before retrying."],
  [/^(?:Refused\b|Provider refused:)/, "The provider refused the operation", "The provider did not allow this operation. Review the details before trying again."],
  [/^(?:PrewriteRefused\b|Input was not sent:)/, "The input could not be sent to the agent runtime", "The runtime did not accept the input. Check the agent before retrying."],
  [/^(?:Superseded\b|Runtime session changed;)/, "The agent session changed and the operation was canceled", "This operation belonged to an earlier session. Check the current session before continuing."],
];

const record = (value: unknown): Record<string, unknown> | null =>
  typeof value === "object" && value !== null ? value as Record<string, unknown> : null;

function serialize(input: unknown): string {
  try { return JSON.stringify(input) ?? String(input); } catch { return String(input); }
}

function unpack(input: unknown): { raw: string; code: string; status: unknown } {
  const object = record(input);
  const raw = input == null ? "" : typeof input === "string" ? input : typeof object?.body === "string" ? object.body
    : input instanceof Error ? input.message : typeof object?.raw === "string" ? object.raw
    : typeof object?.message === "string" ? object.message : serialize(input);
  let parsed = object;
  const start = raw.indexOf("{");
  if (start >= 0) {
    try { parsed = record(JSON.parse(raw.slice(start))) ?? object; } catch { /* Transport messages may not be JSON. */ }
  }
  const detail = record(parsed?.error) ?? parsed;
  return { raw, code: String(object?.code ?? object?.kind ?? detail?.code ?? detail?.kind ?? ""), status: detail?.status ?? object?.status };
}

export function describeError(input: unknown, context: ErrorContext = {}): ErrorDescription {
  const { raw, code, status } = unpack(input);
  const result = (title: string, cause: string, actions: ErrorAction[]): ErrorDescription => ({ title, cause, actions, raw });
  const kind = code.toLowerCase();
  const spawn = /spawn|createprocess|executable|shell/i.test(raw);
  // OS codes are interpreted only in spawn errors; e.g. ENOENT elsewhere may mean an attachment.
  const inferred = !code;
  if (kind === "close_inventory_unavailable" || inferred && /close (?:inventory|status)/i.test(context.operation ?? "")) {
    return result("The close status could not be checked", "Domios could not check which resources are still open. Retry before closing the workspace or Team.", ["retry"]);
  }
  if (kind === "close_failed" || inferred && /close|closing|still open/i.test(context.operation ?? "")) {
    return result("The workspace or Team could not close", "Some resources may still be open. Review the details and retry to finish closing.", ["retry"]);
  }
  if (kind === "workspace_cwd_missing" || inferred && spawn && /os error (?:267|3)\b|enotdir|cwd.*(?:not found|no such|does not exist)/i.test(raw)) {
    return result("The tab's folder no longer exists", context.cwd
      ? `This tab was started in ${context.cwd}, which is no longer available. Choose its new location to continue.`
      : "The tab's starting folder is no longer available. Choose its new location to continue.", ["choose-folder"]);
  }
  if (["attachment_too_large", "payload_too_large"].includes(kind) || inferred && (status === 413 || /HTTP 413\b/.test(raw))) {
    return result("The attachment is too large", "This file exceeds the attachment limit and could not be sent. Choose a smaller file.", ["choose-smaller-file"]);
  }
  if (["provider_auth_lost", "auth_required", "unauthorized", "signed_out"].includes(kind) || inferred && (status === 401 || /\b(?:unauthorized|authentication required|signed.?out|token expired)\b/i.test(raw))) {
    return result("Sign in to continue", "The provider no longer accepts this session's credentials. Sign in again to restore access.", ["sign-in"]);
  }
  if (["provider_access_lost", "access_denied", "forbidden"].includes(kind) || inferred && (status === 403 || /access denied|permission denied|eacces|eperm|os error (?:5|13)\b/i.test(raw))) {
    return result("Access was denied", "Domios could not access the requested resource. Check its permissions before trying again.", ["check-access", "retry"]);
  }
  if (["runtime_deadline", "deadline", "deadline_exceeded"].includes(kind) || inferred && /\bdeadline\b|runtime did not respond in time/i.test(raw)) {
    return result("The agent runtime did not respond in time", "The runtime missed its response deadline. Check the agent's state before trying again.", ["retry"]);
  }
  if (kind === "executable_missing" || inferred && spawn && /enoent|os error 2\b|executable.*(?:not found|missing)/i.test(raw)) {
    return result("The program could not be found", "The shell or agent program is unavailable. Check that it is installed and its path is correct.", ["check-executable", "retry"]);
  }
  const runtime = inferred ? runtimeFailures.find(([pattern]) => pattern.test(raw)) : undefined;
  if (runtime) return result(runtime[1], runtime[2], ["retry"]);
  if (kind === "spawn_failed" || inferred && spawn) {
    return result("The tab could not start", "Domios could not start the shell or agent. Check its program and starting folder, then try again.", ["retry"]);
  }
  return result(`Something went wrong while ${context.operation ?? "completing this operation"}`,
    "The operation could not finish. Review the details before trying again.", ["retry"]);
}
