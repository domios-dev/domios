import type { SourceFile } from "typescript/unstable/ast";
export function parseTypeScriptSources(sources: Map<string, string>): Map<string, SourceFile>;
export function parseTypeScriptSource(file: string, source: string): SourceFile;
