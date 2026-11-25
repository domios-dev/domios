export interface ProviderErrorCommand {
  command: string;
  label?: string;
}

export interface ProviderErrorRemediation {
  code: string;
  title: string;
  message: string;
  commands?: ProviderErrorCommand[];
  retry?: boolean;
  rawDetail?: string;
}

function boundedText(value: unknown, maxChars: number): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 && trimmed.length <= maxChars ? trimmed : undefined;
}

export function parseProviderErrorRemediation(toolInput: unknown): ProviderErrorRemediation | null {
  if (!toolInput || typeof toolInput !== "object" || Array.isArray(toolInput)) return null;
  const raw = (toolInput as Record<string, unknown>).remediation;
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const candidate = raw as Record<string, unknown>;
  const code = boundedText(candidate.code, 128);
  const title = boundedText(candidate.title, 256);
  const message = boundedText(candidate.message, 2048);
  if (!code || !title || !message) return null;

  let commands: ProviderErrorCommand[] | undefined;
  if (Array.isArray(candidate.commands)) {
    const parsed: ProviderErrorCommand[] = [];
    for (const entry of candidate.commands.slice(0, 4)) {
      if (!entry || typeof entry !== "object" || Array.isArray(entry)) continue;
      const item = entry as Record<string, unknown>;
      const command = boundedText(item.command, 512);
      if (!command) continue;
      const label = boundedText(item.label, 160);
      parsed.push(label ? { command, label } : { command });
    }
    if (parsed.length > 0) commands = parsed;
  }

  const retry = candidate.retry === true ? true : undefined;
  const rawDetail = boundedText(candidate.rawDetail, 4096);
  return {
    code,
    title,
    message,
    ...(commands ? { commands } : {}),
    ...(retry ? { retry } : {}),
    ...(rawDetail ? { rawDetail } : {}),
  };
}

export function hasRetryableProviderErrorRemediation(toolInput: unknown): boolean {
  return parseProviderErrorRemediation(toolInput)?.retry === true;
}
