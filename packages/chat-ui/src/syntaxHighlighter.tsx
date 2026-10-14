import React, { useEffect, useRef, useState } from "react";
import { PrismLight } from "react-syntax-highlighter";
import bash from "react-syntax-highlighter/dist/esm/languages/prism/bash";
import c from "react-syntax-highlighter/dist/esm/languages/prism/c";
import cpp from "react-syntax-highlighter/dist/esm/languages/prism/cpp";
import csharp from "react-syntax-highlighter/dist/esm/languages/prism/csharp";
import css from "react-syntax-highlighter/dist/esm/languages/prism/css";
import diff from "react-syntax-highlighter/dist/esm/languages/prism/diff";
import go from "react-syntax-highlighter/dist/esm/languages/prism/go";
import java from "react-syntax-highlighter/dist/esm/languages/prism/java";
import javascript from "react-syntax-highlighter/dist/esm/languages/prism/javascript";
import json from "react-syntax-highlighter/dist/esm/languages/prism/json";
import jsx from "react-syntax-highlighter/dist/esm/languages/prism/jsx";
import markdown from "react-syntax-highlighter/dist/esm/languages/prism/markdown";
import markup from "react-syntax-highlighter/dist/esm/languages/prism/markup";
import powershell from "react-syntax-highlighter/dist/esm/languages/prism/powershell";
import python from "react-syntax-highlighter/dist/esm/languages/prism/python";
import rust from "react-syntax-highlighter/dist/esm/languages/prism/rust";
import scss from "react-syntax-highlighter/dist/esm/languages/prism/scss";
import sql from "react-syntax-highlighter/dist/esm/languages/prism/sql";
import swift from "react-syntax-highlighter/dist/esm/languages/prism/swift";
import toml from "react-syntax-highlighter/dist/esm/languages/prism/toml";
import tsx from "react-syntax-highlighter/dist/esm/languages/prism/tsx";
import typescript from "react-syntax-highlighter/dist/esm/languages/prism/typescript";
import yaml from "react-syntax-highlighter/dist/esm/languages/prism/yaml";

import { createElement } from "react-syntax-highlighter";
import { BudgetedHighlightRows, useHighlightVisibility } from "./ChatHighlightBudget";
import { highlightSourceEligible } from "./highlightBudget";
import { DEV_BUILD } from "../../../src/lib/devBuild";
import { createGrammarLoader, type GrammarLoader, type GrammarLoadResult } from "./syntaxGrammarLoader";
export { createElement } from "react-syntax-highlighter";

type Grammar = { displayName: string; aliases?: readonly string[] };

// Registering all ~300 Prism grammars costs ~180 ms at startup; the rest load on first use of a language outside this set.
const EAGER_GRAMMARS: Grammar[] = [
  bash, c, cpp, csharp, css, diff, go, java, javascript, json, jsx, markdown, markup,
  powershell, python, rust, scss, sql, swift, toml, tsx, typescript, yaml,
];
const eagerNames = new Set<string>();
for (const grammarModule of EAGER_GRAMMARS) {
  // Node's CommonJS interop hands over the module namespace instead of its default export.
  const grammar = (grammarModule as Grammar & { default?: Grammar }).default ?? grammarModule;
  PrismLight.registerLanguage(grammar.displayName, grammar);
  eagerNames.add(grammar.displayName);
  for (const alias of grammar.aliases ?? []) eagerNames.add(alias);
}

let allGrammarsLoaded = false;
export const loadAllGrammars = createGrammarLoader(async () => {
  await import("react-syntax-highlighter/dist/esm/prism");
  allGrammarsLoaded = true;
}, (error) => {
  if (DEV_BUILD) console.debug("[chat-highlight] Grammar load failed; showing plain text", error);
});

function needsGrammar(language: string | undefined): language is string {
  return !!language && language !== "text" && !allGrammarsLoaded && !eagerNames.has(language);
}

type PrismLightProps = React.ComponentProps<typeof PrismLight>;

export function SyntaxHighlighter({ explicitHighlight = false, loadGrammars = loadAllGrammars, ...props }: PrismLightProps & { explicitHighlight?: boolean; loadGrammars?: GrammarLoader }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const visible = useHighlightVisibility(hostRef, explicitHighlight);
  const source = Array.isArray(props.children) ? props.children.join("") : props.children;
  const enabled = visible && highlightSourceEligible(source, explicitHighlight);
  const [loadResult, setLoadResult] = useState<{ language: string; result: GrammarLoadResult } | null>(null);
  const requestedLanguage = enabled ? props.language : "text";
  const failed = loadResult?.language === requestedLanguage && loadResult?.result === "failed";
  const language = failed ? "text" : requestedLanguage;
  useEffect(() => {
    if (!needsGrammar(requestedLanguage)) return;
    let live = true;
    void loadGrammars().then((result) => {
      if (live) setLoadResult({ language: requestedLanguage, result });
    });
    return () => { live = false; };
  }, [requestedLanguage, loadGrammars]);
  return <><div ref={hostRef} style={{ display: "contents" }}><PrismLight {...props} language={language}
    wrapLines={props.renderer ? props.wrapLines : props.wrapLines ?? false}
    renderer={(arguments_) => <BudgetedHighlightRows rows={arguments_.rows} enabled={enabled}
      plain={() => props.renderer
        ? props.renderer({ ...arguments_, rows: arguments_.rows.map((row) => ({ ...row, children: [{ type: "text", value: plainHighlightText(row) }] })) })
        : source}
      render={() => props.renderer ? props.renderer(arguments_) : arguments_.rows.map((node, index) => createElement({
        node, stylesheet: arguments_.stylesheet, useInlineStyles: arguments_.useInlineStyles, key: index,
      }))} />} /></div>{failed && <span role="status">Syntax highlighting unavailable; showing plain text.</span>}</>;
}

interface PlainHighlightNode { value?: string | number; children?: readonly PlainHighlightNode[] }

function plainHighlightText(node: PlainHighlightNode): string {
  if (node.value != null) return String(node.value);
  return (node.children ?? []).map(plainHighlightText).join("");
}
