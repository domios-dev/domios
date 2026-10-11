import ts from "typescript";

export interface TransportViolation {
  file: string;
  line: number;
  kind: "tauri-import" | "invoke" | "audited-command" | "native-global";
  target: string;
}

export function scanTransportSource(file: string, source: string): TransportViolation[] {
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, file.endsWith("tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const violations: TransportViolation[] = [];
  const invokes = new Map<string, "invoke" | "audited-command">();
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
          if ((binding.propertyName ?? binding.name).text === "invoke") invokes.set(binding.name.text, module === "@tauri-apps/api/core" ? "invoke" : "audited-command");
        }
      }
    }
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && invokes.has(node.expression.text)) {
      const command = node.arguments[0];
      add(node, invokes.get(node.expression.text)!, command && ts.isStringLiteral(command) ? command.text : "<dynamic>");
    }
    const markerAssignment = node.parent && ts.isBinaryExpression(node.parent) && node.parent.left === node
      && node.parent.operatorToken.kind === ts.SyntaxKind.EqualsToken
      && ts.isObjectLiteralExpression(node.parent.right) && node.parent.right.properties.length === 0;
    if (!markerAssignment && ((ts.isPropertyAccessExpression(node) && ["__TAURI_INTERNALS__", "__TAURI__"].includes(node.name.text))
      || (ts.isElementAccessExpression(node) && node.argumentExpression && ts.isStringLiteral(node.argumentExpression)
        && ["__TAURI_INTERNALS__", "__TAURI__"].includes(node.argumentExpression.text)))) {
      add(node, "native-global", "direct native runtime access");
    }
    ts.forEachChild(node, visit);
  };
  visit(ast);
  return violations;
}
