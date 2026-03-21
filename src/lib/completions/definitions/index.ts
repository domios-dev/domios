import type { CompletionDefinition } from "../types";
import claudeDef from "./claude.json";
import codetermDef from "./codeterm.json";

export const defaultDefinitions: CompletionDefinition[] = [
  claudeDef as CompletionDefinition,
  codetermDef as CompletionDefinition,
];
