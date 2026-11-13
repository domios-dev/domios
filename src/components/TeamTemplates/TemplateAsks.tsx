import type { ReactNode } from "react";
import { Globe, GitBranch, KeyRound, SquareTerminal, Wrench, MonitorSmartphone } from "lucide-react";
import { roleDisplayLabel } from "../../lib/roleKind";
import { DropdownSelect } from "../shared/DropdownMenu";
import { openSecretsSettings } from "../../lib/teamTemplates/vaultSecrets";
import { visibleAsks, type ShownAsk } from "../../lib/teamTemplates/startModel";

const ADD_SECRET = "\u0000add";

export interface SecretBinding { bindings: Record<string, string>; vault: readonly string[] | null; onBind: (name: string, entry: string) => void }

/** A secret ask binds to a local vault entry by name; the value never enters the form. */
function SecretSelect({ ask, binding }: { ask: { name: string; required: boolean }; binding: SecretBinding }) {
  // "Choose a secret" is the placeholder of a required ask, never an option; an optional ask can be skipped.
  const options = [
    ...(ask.required ? [] : [{ value: "", label: "Skip" }]),
    ...(binding.vault ?? []).map((name) => ({ value: name, label: name })),
    { value: ADD_SECRET, label: "Add a secret…" },
  ];
  return <div className="w-[170px] shrink-0">
    <DropdownSelect aria-label={`Secret for ${ask.name}`} options={options} value={binding.bindings[ask.name] ?? ""}
      placeholder="Choose a secret"
      disabled={binding.vault === null} onChange={(value) => value === ADD_SECRET ? openSecretsSettings() : binding.onBind(ask.name, value)} />
  </div>;
}

type TemplateAsk = import("../../../gen/TemplateAsk").TemplateAsk;

function askRow(ask: ShownAsk): { icon: ReactNode; title: string; detail?: string } {
  switch (ask.kind) {
    case "network": return { icon: <Globe size={13} />, title: "Network", detail: ask.hosts?.length ? ask.hosts.join(", ") : "any host" };
    case "repo_write": return { icon: <GitBranch size={13} />, title: "Edits the project" };
    case "shell": return { icon: <SquareTerminal size={13} />, title: "Runs shell commands" };
    case "browser": return { icon: <MonitorSmartphone size={13} />, title: "Uses a browser" };
    case "secret": return { icon: <KeyRound size={13} />, title: ask.name, detail: ask.required ? ask.purpose : `${ask.purpose} · optional` };
    case "tool": return { icon: <Wrench size={13} />, title: ask.tool, detail: `for the ${roleDisplayLabel(ask.role)}` };
  }
}

/** Only the real asks a package makes; absent asks are never shown. A secret's purpose wraps under its name, beside its select. */
export function TemplateAsks({ asks, secrets, title = "Asks for" }: { asks: readonly TemplateAsk[]; secrets?: SecretBinding; title?: string }) {
  const shown = visibleAsks(asks);
  if (shown.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-1">
      <div className="text-[11px] text-[var(--ct-muted-fg)]">{title}</div>
      <ul className="flex flex-col gap-1.5">
        {shown.map((ask, index) => {
          const row = askRow(ask);
          if (ask.kind === "secret") {
            return (
              <li key={`${ask.kind}:${index}`} className="flex flex-wrap items-start gap-2 text-[12px]" data-ask={ask.kind}>
                <span aria-hidden="true" className="self-start pt-0.5 text-[var(--ct-muted-fg)]">{row.icon}</span>
                <span className="min-w-0 flex-1">
                  <span className="block font-mono">{row.title}</span>
                  {row.detail && <span className="block text-[11px] leading-snug text-[var(--ct-muted-fg)]">{row.detail}</span>}
                </span>
                {secrets && <SecretSelect ask={ask} binding={secrets} />}
              </li>
            );
          }
          return (
            <li key={`${ask.kind}:${index}`} className="flex flex-wrap items-start gap-2 text-[12px]" data-ask={ask.kind}>
              <span aria-hidden="true" className="text-[var(--ct-muted-fg)]">{row.icon}</span>
              <span className="min-w-0 break-words [overflow-wrap:anywhere]">{row.title}</span>
              {row.detail && <span className="min-w-0 break-words text-[var(--ct-muted-fg)] [overflow-wrap:anywhere]">· {row.detail}</span>}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
