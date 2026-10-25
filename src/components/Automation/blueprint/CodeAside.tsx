import type { BlueprintIrNode, BlueprintValue } from "../../../lib/blueprintApi";
import { codeOutline } from "./codeOutline";
import { portType } from "./graphModel";

function plain(value: BlueprintValue | undefined): string {
  if (value && typeof value === "object" && !Array.isArray(value) && "lit" in value) return plain((value as Readonly<Record<string, BlueprintValue>>).lit);
  return value === undefined || value === null ? "" : typeof value === "object" ? JSON.stringify(value) : String(value);
}

function limitsOf(definition: BlueprintIrNode): [string, string][] {
  const limits = definition.code?.limits;
  if (!limits || typeof limits !== "object" || Array.isArray(limits)) return [];
  return Object.entries(limits as Readonly<Record<string, BlueprintValue>>).map(([name, value]) => [name.replace(/_/g, " "), plain(value)]);
}

/** What a reader wants beside a node's code: its language, the symbols it defines, its typed ports and its limits. */
export function CodeAside({ definition, language, languageLabel, source }: { definition: BlueprintIrNode; language: string | undefined; languageLabel: string; source: string }) {
  const outline = codeOutline(source, language);
  const limits = limitsOf(definition);
  const caps = definition.code?.caps ?? definition.caps ?? [];
  return <>
    <section className="ct-code-aside__group">
      <h4 className="ct-code-aside__heading">{languageLabel}</h4>
      <p className="ct-code-aside__muted">{definition.name ?? definition.id}{definition.does ? ` · ${definition.does}` : ""}</p>
    </section>
    {outline.length > 0 && <section className="ct-code-aside__group" aria-label="Outline">
      <h4 className="ct-code-aside__heading">Outline</h4>
      <ul className="ct-code-aside__outline">{outline.map((symbol, index) => <li key={index}>
        <span className="ct-code-aside__kind" data-kind={symbol.kind}>{symbol.kind === "class" ? "class" : "fn"}</span>
        <code><span className="ct-code-aside__name">{symbol.name}</span>{symbol.kind === "function" ? `(${symbol.params})` : ""}{symbol.returns ? ` → ${symbol.returns}` : ""}</code>
      </li>)}</ul>
    </section>}
    {(definition.in || definition.out) && <section className="ct-code-aside__group" aria-label="Ports">
      <h4 className="ct-code-aside__heading">Ports</h4>
      <dl className="ct-code-aside__facts">
        {definition.in && <><dt>Input</dt><dd><code>{portType(definition.in)}</code></dd></>}
        {definition.out && <><dt>Output</dt><dd><code>{portType(definition.out)}</code></dd></>}
      </dl>
    </section>}
    {(limits.length > 0 || caps.length > 0) && <section className="ct-code-aside__group" aria-label="Limits">
      <h4 className="ct-code-aside__heading">Limits</h4>
      <dl className="ct-code-aside__facts">
        {limits.map(([name, value]) => <div key={name} className="contents"><dt>{name}</dt><dd><code>{value}</code></dd></div>)}
        {caps.length > 0 && <><dt>Capabilities</dt><dd>{caps.join(", ")}</dd></>}
      </dl>
    </section>}
  </>;
}
