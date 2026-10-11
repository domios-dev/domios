import ts from "typescript";

export interface TransportViolation {
  file: string;
  line: number;
  kind: "tauri-import" | "invoke";
  target: string;
}

export function scanTransportSource(file: string, source: string): TransportViolation[] {
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, file.endsWith("tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const violations: TransportViolation[] = [];
  const invokes = new Set<string>();
  const add = (node: ts.Node, kind: TransportViolation["kind"], target: string) => {
    violations.push({ file, line: ast.getLineAndCharacterOfPosition(node.getStart(ast)).line + 1, kind, target });
  };
  const moduleName = (node: ts.Node): string | undefined => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      return node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier) ? node.moduleSpecifier.text : undefined;
    }
    if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword
      || ts.isIdentifier(node.expression) && node.expression.text === "require")) {
      return node.arguments[0] && ts.isStringLiteral(node.arguments[0]) ? node.arguments[0].text : undefined;
    }
    return undefined;
  };
  const visit = (node: ts.Node) => {
    const module = moduleName(node);
    if (module?.startsWith("@tauri-apps/")) add(node, "tauri-import", module);
    if (ts.isImportDeclaration(node) && module && (/bridge$/.test(module) || module === "@tauri-apps/api/core")) {
      const bindings = node.importClause?.namedBindings;
      if (bindings && ts.isNamedImports(bindings)) {
        for (const binding of bindings.elements) {
          if ((binding.propertyName ?? binding.name).text === "invoke") invokes.add(binding.name.text);
        }
      }
    }
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && invokes.has(node.expression.text)) {
      const command = node.arguments[0];
      add(node, "invoke", command && ts.isStringLiteral(command) ? command.text : "<dynamic>");
    }
    ts.forEachChild(node, visit);
  };
  visit(ast);
  return violations;
}

export function transportViolationKey(site: TransportViolation): string {
  return `${site.file}:${site.kind}:${site.target}`;
}
