import React, { useEffect, useState } from "react";
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
let allGrammars: Promise<boolean> | null = null;

// The full Prism build registers every grammar into the refractor instance PrismLight shares.
function loadAllGrammars(): Promise<boolean> {
  allGrammars ??= import("react-syntax-highlighter/dist/esm/prism").then(() => {
    allGrammarsLoaded = true;
    return true;
  }, () => {
    allGrammars = null;
    return false;
  });
  return allGrammars;
}

function needsGrammar(language: string | undefined): language is string {
  return !!language && language !== "text" && !allGrammarsLoaded && !eagerNames.has(language);
}

type PrismLightProps = React.ComponentProps<typeof PrismLight>;

export function SyntaxHighlighter(props: PrismLightProps) {
  const { language } = props;
  const [, setLoaded] = useState(0);
  useEffect(() => {
    if (!needsGrammar(language)) return;
    let live = true;
    void loadAllGrammars().then((ok) => {
      if (live && ok) setLoaded((n) => n + 1);
    });
    return () => { live = false; };
  }, [language]);
  return <PrismLight {...props} />;
}
