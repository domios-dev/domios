import type { ReactNode } from "react";
import { Globe, GitBranch, KeyRound, SquareTerminal, Wrench, MonitorSmartphone, FileCode } from "lucide-react";
import { roleDisplayLabel } from "../../lib/roleKind";
import { DropdownSelect } from "../shared/DropdownMenu";
import { openSecretsSettings } from "../../lib/teamTemplates/vaultSecrets";

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
  return <DropdownSelect className="ml-auto w-44 shrink-0" aria-label={`Secret for ${ask.name}`} options={options} value={binding.bindings[ask.name] ?? ""}
    placeholder="Choose a secret"
    disabled={binding.vault === null} onChange={(value) => value === ADD_SECRET ? openSecretsSettings() : binding.onBind(ask.name, value)} />;
}

type TemplateAsk = import("../../../gen/TemplateAsk").TemplateAsk;

function askRow(ask: TemplateAsk): { icon: ReactNode; title: string; detail?: string } {
  switch (ask.kind) {
    case "network": return { icon: <Globe size={13} />, title: "Network", detail: ask.hosts?.length ? ask.hosts.join(", ") : "any host" };
    case "repo_write": return { icon: <GitBranch size={13} />, title: "Edits the project" };
    case "shell": return { icon: <SquareTerminal size={13} />, title: "Runs shell commands" };
    case "browser": return { icon: <MonitorSmartphone size={13} />, title: "Uses a browser" };
    case "secret": return { icon: <KeyRound size={13} />, title: ask.name, detail: ask.required ? ask.purpose : `${ask.purpose} · optional` };
    case "tool": return { icon: <Wrench size={13} />, title: ask.tool, detail: `for the ${roleDisplayLabel(ask.role)}` };
    case "script": return { icon: <FileCode size={13} />, title: "Runs scripts", detail: ask.item };
  }
}

/** Only the real asks a package makes; absent asks are never shown. */
export function TemplateAsks({ asks, secrets }: { asks: readonly TemplateAsk[]; secrets?: SecretBinding }) {
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
              {ask.kind === "secret" && secrets && <SecretSelect ask={ask} binding={secrets} />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
