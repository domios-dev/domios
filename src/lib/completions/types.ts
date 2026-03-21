export interface CompletionDefinition {
  command: string;
  subcommands?: SubcommandDef[];
  flags?: FlagDef[];
}

export interface SubcommandDef {
  name: string;
  description: string;
  flags?: FlagDef[];
  subcommands?: SubcommandDef[];
}

export interface FlagDef {
  name: string;
  short?: string;
  description: string;
  takesValue?: boolean;
  values?: string[];
}

export interface CompletionItem {
  text: string;
  display: string;
  description: string;
  kind: "subcommand" | "flag" | "value";
}

export interface ParsedInput {
  command: string;
  args: string[];
  currentArg: string;
}
