import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import ts from "typescript";

function bypasses(source: string, filename = "fixture.tsx"): string[] {
  const tree = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const folderNames = new Set(["WorkspaceFolderError"]);
  const noticeNames = new Set(["ErrorNotice"]);
  for (const statement of tree.statements) {
    if (!ts.isImportDeclaration(statement)) continue;
    const bindings = statement.importClause?.namedBindings;
    if (!bindings || !ts.isNamedImports(bindings)) continue;
    for (const binding of bindings.elements) {
      const exported = binding.propertyName?.text ?? binding.name.text;
      if (exported === "WorkspaceFolderError") folderNames.add(binding.name.text);
      if (exported === "ErrorNotice") noticeNames.add(binding.name.text);
    }
  }
  const found: string[] = [];
  function visit(node: ts.Node) {
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = node.tagName.getText(tree);
      const name = tag.split(".").slice(-1)[0];
      if (folderNames.has(name)) found.push(tag);
      if (noticeNames.has(name)) {
        const attributes = node.attributes.properties;
        const lastBlocking = [...attributes].reverse().find(attribute => ts.isJsxAttribute(attribute) && attribute.name.getText(tree) === "blocking");
        const explicitlyFalse = lastBlocking && ts.isJsxAttribute(lastBlocking) && lastBlocking.initializer
          && ts.isJsxExpression(lastBlocking.initializer) && lastBlocking.initializer.expression?.kind === ts.SyntaxKind.FalseKeyword;
        const spreads = attributes.filter(ts.isJsxSpreadAttribute);
        const safelyDisabled = explicitlyFalse && spreads.every(spread => spread.pos < lastBlocking.pos);
        if (!safelyDisabled && (lastBlocking || spreads.length)) found.push(tag);
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(tree);
  return found;
}

test("blocking pane inventory has no legacy folder or blocking ErrorNotice renders", () => {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
  const found: string[] = [];
  function walk(dir: string) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
        for (const tag of bypasses(readFileSync(full, "utf8"), full)) found.push(`${path.relative(root, full)}: ${tag}`);
      }
    }
  }
  walk(root);
  assert.deepEqual(found, []);
});

test("inventory detects aliases, multiline and dynamic blocking props without flagging inline notices", () => {
  assert.deepEqual(bypasses('import { WorkspaceFolderError as Old, ErrorNotice as Notice } from "legacy"; const x = <><Old /><Notice\n blocking={pending} /><Notice {...{ blocking: true }} /><Notice blocking={false} /><Notice {...props} /><InlineErrorNotice /></>;'), ["Old", "Notice", "Notice", "Notice"]);
});
