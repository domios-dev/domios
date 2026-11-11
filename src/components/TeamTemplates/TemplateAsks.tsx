import type { ReactNode } from "react";
import { Globe, GitBranch, KeyRound, SquareTerminal, Wrench, MonitorSmartphone, FileCode } from "lucide-react";
import { kindDisplayName } from "../../lib/roleKind";

type TemplateAsk = import("../../../gen/TemplateAsk").TemplateAsk;

function askRow(ask: TemplateAsk): { icon: ReactNode; title: string; detail?: string } {
  switch (ask.kind) {
    case "network": return { icon: <Globe size={13} />, title: "Network", detail: ask.hosts?.length ? ask.hosts.join(", ") : "any host" };
    case "repo_write": return { icon: <GitBranch size={13} />, title: "Edits the project" };
    case "shell": return { icon: <SquareTerminal size={13} />, title: "Runs shell commands" };
    case "browser": return { icon: <MonitorSmartphone size={13} />, title: "Uses a browser" };
    case "secret": return { icon: <KeyRound size={13} />, title: ask.name, detail: ask.required ? ask.purpose : `${ask.purpose} · optional` };
    case "tool": return { icon: <Wrench size={13} />, title: ask.tool, detail: `for the ${kindDisplayName(ask.role)}` };
    case "script": return { icon: <FileCode size={13} />, title: "Runs scripts", detail: ask.item };
  }
}

/** Only the real asks a package makes; absent asks are never shown. */
export function TemplateAsks({ asks }: { asks: readonly TemplateAsk[] }) {
  if (asks.length === 0) return null;
  return (
    <section aria-label="Asks for" className="flex flex-col gap-1">
      <div className="text-[11px] text-[var(--ct-muted-fg)]">Asks for</div>
      <ul className="flex flex-col gap-1.5">
        {asks.map((ask, index) => {
          const row = askRow(ask);
          return (
            <li key={`${ask.kind}:${index}`} className="flex items-center gap-2 text-[12px]" data-ask={ask.kind}>
              <span aria-hidden="true" className="text-[var(--ct-muted-fg)]">{row.icon}</span>
              <span className={ask.kind === "secret" ? "font-mono" : ""}>{row.title}</span>
              {row.detail && <span className="truncate text-[var(--ct-muted-fg)]">· {row.detail}</span>}
              {ask.kind === "script" && <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-[var(--ct-yellow,#d97706)]" />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
