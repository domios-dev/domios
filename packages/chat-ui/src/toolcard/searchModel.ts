import { parseShellComposition, toolCommand, type ToolEvent } from "@codeterm/chat-core";

function searchInput(event: ToolEvent): Record<string, unknown> {
  if (event.message.toolInput) return event.message.toolInput;
  try {
    const input: unknown = JSON.parse(event.message.toolArgs ?? "{}");
    return input && typeof input === "object" && !Array.isArray(input) ? input as Record<string, unknown> : {};
  } catch {
    return {};
  }
}

function shellSubject(event: ToolEvent): { subject?: string; filesOnly: boolean } {
  const command = toolCommand(event);
  const segments = parseShellComposition(command.command, command.argv)?.segments ?? [];
  const argv = segments.find(segment => /(?:^|[/\\])(?:rg|grep)(?:\.exe)?$/.test(segment.argv[0] ?? ""))?.argv ?? [];
  const filesOnly = argv.includes("--files");
  for (let i = 1; i < argv.length; i++) {
    const word = argv[i];
    if (["-e", "--regexp", ...(filesOnly ? ["-g", "--glob"] : [])].includes(word)) return { subject: argv[i + 1], filesOnly };
    if (["-g", "--glob", "-t", "--type", "--type-not", "-A", "-B", "-C", "-m", "--max-count", "--encoding"].includes(word)) { i++; continue; }
    if (word === "--") return { subject: argv[i + 1], filesOnly };
    if (!word.startsWith("-")) return { subject: word, filesOnly };
  }
  return { filesOnly };
}

function quantity(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : noun === "match" ? "es" : "s"}`;
}

export function searchModel(event: ToolEvent): { title: string; subject: string; matches: number; files?: number; counts: string; fact: string } {
  const input = searchInput(event);
  const shell = shellSubject(event);
  const filesOnly = /(?:^|[._])glob$/i.test(event.message.toolName ?? "") || shell.filesOnly;
  const fileResults = filesOnly || input.output_mode === "files_with_matches";
  const subject = [input.pattern, input.query, shell.subject].find((value): value is string => typeof value === "string" && !!value.trim()) ?? "";
  const text = [event.summary, event.result].filter(Boolean).join("\n");
  const explicitMatches = text.match(/(\d+)\s+(?:match(?:es)?|results?)\b/i)?.[1];
  const explicitFiles = text.match(/(\d+)\s+files?\b/i)?.[1];
  const lines = (event.result ?? "").split("\n").map(line => line.trim()).filter(line => line && !/^(?:Script completed|Wall time .+ seconds|Output:|No (?:matches|files).*)$/i.test(line));
  const matches = explicitMatches === undefined ? new Set(lines).size : Number(explicitMatches);
  const paths = new Set(lines.map(line => line.match(/^(.+?):\d+(?::\d+)?:/)?.[1]).filter(Boolean));
  const fileCount = explicitFiles === undefined ? (fileResults ? new Set(lines).size : paths.size) : Number(explicitFiles);
  const files = fileCount > 0 ? fileCount : undefined;
  const fact = event.status === "pending" ? "" : fileResults ? (files ? quantity(files, "file") : "No files") : quantity(matches, "match");
  const counts = fileResults || !fact ? fact : [fact, files && quantity(files, "file")].filter(Boolean).join(" · ");
  return { title: filesOnly ? "Find files" : "Search", subject, matches, files, counts, fact };
}
