import type { ReasoningEffortInfo } from "@codeterm/plugin-sdk";

export function variantLaunch(args: string[]): { args: string[]; config: Record<string, unknown> } {
  const retained: string[] = [];
  let variant: string | undefined;
  let agent: string | undefined;
  for (let index = 0; index < args.length; index++) {
    const arg = String(args[index]);
    if (arg === "--variant") { variant = String(args[++index] ?? ""); continue; }
    if (arg.startsWith("--variant=")) { variant = arg.slice(10); continue; }
    if (arg === "--agent") agent = String(args[index + 1] ?? "");
    if (arg.startsWith("--agent=")) agent = arg.slice(8);
    retained.push(arg);
  }
  if (!variant) return { args: retained, config: {} };
  if (!agent) { agent = "build"; retained.push("--agent", agent); }
  return { args: retained, config: { agent: { [agent]: { variant } } } };
}

export function parseModelVariants(stdout: string): Map<string, ReasoningEffortInfo[]> {
  const result = new Map<string, ReasoningEffortInfo[]>();
  let id: string | null = null;
  let lines: string[] = [];
  function finish() {
    if (!id || lines.length === 0) return;
    try {
      const model: unknown = JSON.parse(lines.join("\n"));
      if (!model || typeof model !== "object") return;
      const variants = (model as { variants?: unknown }).variants;
      if (!variants || typeof variants !== "object" || Array.isArray(variants)) {
        result.set(id, []);
        return;
      }
      const efforts = Object.entries(variants).filter(([name, value]) =>
        name !== "default" && /^[a-zA-Z0-9_.-]+$/.test(name) && value !== null && typeof value === "object"
        && !(value as { disabled?: boolean }).disabled,
      ).map(([name]) => ({ id: name, displayName: name[0].toUpperCase() + name.slice(1) }));
      result.set(id, efforts.length ? [{ id: "default", displayName: "Default" }, ...efforts] : []);
    } catch { }
  }
  for (const line of stdout.split("\n")) {
    if (/^[a-z0-9][a-z0-9_.-]*\/[a-z0-9][a-z0-9_.-]*$/i.test(line.trim())) {
      finish();
      id = line.trim();
      lines = [];
    } else if (id) lines.push(line);
  }
  finish();
  return result;
}
