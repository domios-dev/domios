import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

export const fieldValidation = new Map([
  ["src/components/shared/FormRow.tsx:error", "A short validation hint belongs to the labelled input."],
  ["src/components/shared/TextPromptDialog.tsx:error", "The supplied synchronous validator describes the text input."],
  ["src/components/Extensions/PluginNetworkAccess.tsx:error", "The host parser validates the adjacent allow-host input."],
  ["src/components/Settings/SchemaRenderer.tsx:error", "The JSON object input reports syntax and object-shape validation."],
]);
const errorName = name => /error|failure|(?:^|[._])fault/i.test(name) && !/^(?:is|has|on|render|show)[A-Z]/.test(name) && !/\.errors$/.test(name);
const diagnosticName = name => /(?:^|\.)(?:message|reason|detail|details|stack|text|raw)$/.test(name);

function renderedError(expression, diagnosticContext, jsxBindings, rawBindings) {
  if (!expression) return false;
  if (ts.isJsxElement(expression) || ts.isJsxSelfClosingElement(expression) || ts.isJsxFragment(expression)) return false;
  if (ts.isIdentifier(expression)) return !jsxBindings.has(expression.text) && ((errorName(expression.text) || rawBindings.has(expression.text)) || diagnosticContext && diagnosticName(expression.text));
  if (ts.isPropertyAccessExpression(expression)) return errorName(expression.getText()) || rawBindings.has(expression.expression.getText()) && diagnosticName(expression.getText()) || diagnosticContext && diagnosticName(expression.getText());
  if (ts.isParenthesizedExpression(expression) || ts.isNonNullExpression(expression) || ts.isAsExpression(expression)) return renderedError(expression.expression, diagnosticContext, jsxBindings, rawBindings);
  if (ts.isConditionalExpression(expression)) return renderedError(expression.whenTrue, diagnosticContext, jsxBindings, rawBindings) || renderedError(expression.whenFalse, diagnosticContext, jsxBindings, rawBindings);
  if (ts.isBinaryExpression(expression)) return expression.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken
    ? renderedError(expression.right, diagnosticContext, jsxBindings, rawBindings)
    : renderedError(expression.left, diagnosticContext, jsxBindings, rawBindings) || renderedError(expression.right, diagnosticContext, jsxBindings, rawBindings);
  if (ts.isTemplateExpression(expression)) return expression.templateSpans.some(span => renderedError(span.expression, diagnosticContext, jsxBindings, rawBindings));
  if (ts.isCallExpression(expression)) return expression.arguments.some(arg => renderedError(arg, diagnosticContext, jsxBindings, rawBindings));
  return false;
}

function errorContext(node) {
  for (let current = node.parent; current; current = current.parent) {
    if (!ts.isJsxElement(current)) continue;
    if (current.openingElement.attributes.properties.some(attribute => ts.isJsxAttribute(attribute)
      && ((attribute.name.getText() === "role" && /alert|error/.test(attribute.initializer?.getText() ?? ""))
        || attribute.name.getText() === "className" && /(?:^|[-_])error/.test(attribute.initializer?.getText() ?? "")))) return true;
  }
  return false;
}

export function rawErrorRenders(source, filename, exemptions = fieldValidation) {
  const file = filename.replaceAll("\\", "/");
  if (file === "src/components/shared/ErrorNotice.tsx") return [];
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const jsxBindings = new Set();
  function hasJsx(node) {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node) || ts.isJsxFragment(node)) return true;
    return ts.isConditionalExpression(node) && (hasJsx(node.whenTrue) || hasJsx(node.whenFalse));
  }
  function bindings(node) {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer && hasJsx(node.initializer)) jsxBindings.add(node.name.text);
    ts.forEachChild(node, bindings);
  }
  bindings(tree);
  const setters = new Map();
  const rawBindings = new Set();
  function stateBindings(node) {
    if (ts.isVariableDeclaration(node) && ts.isArrayBindingPattern(node.name)) {
      const [value, setter] = node.name.elements;
      if (value && setter && ts.isBindingElement(value) && ts.isBindingElement(setter)
        && ts.isIdentifier(value.name) && ts.isIdentifier(setter.name)) setters.set(setter.name.text, value.name.text);
    }
    ts.forEachChild(node, stateBindings);
  }
  function mentionsError(node, caught) {
    if (ts.isIdentifier(node) && (errorName(node.text) || caught.has(node.text))) return true;
    let found = false;
    ts.forEachChild(node, child => { if (mentionsError(child, caught)) found = true; });
    return found;
  }
  function rawState(node) {
    const caught = new Set();
    for (let ancestor = node.parent; ancestor; ancestor = ancestor.parent) {
      if (ts.isCatchClause(ancestor) && ancestor.variableDeclaration) caught.add(ancestor.variableDeclaration.name.getText());
      if ((ts.isArrowFunction(ancestor) || ts.isFunctionExpression(ancestor)) && ts.isCallExpression(ancestor.parent)
        && ts.isPropertyAccessExpression(ancestor.parent.expression)
        && (ancestor.parent.expression.name.text === "catch" || ancestor.parent.expression.name.text === "then" && ancestor.parent.arguments[1] === ancestor)) {
        for (const parameter of ancestor.parameters) caught.add(parameter.name.getText());
      }
    }
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && setters.has(node.expression.text)
      && node.arguments.some(argument => mentionsError(argument, caught))) rawBindings.add(setters.get(node.expression.text));
    ts.forEachChild(node, rawState);
  }
  stateBindings(tree);
  rawState(tree);

  const found = [];
  function visit(node) {
    if (ts.isJsxExpression(node) && !ts.isJsxAttribute(node.parent) && node.expression
      && renderedError(node.expression, errorContext(node), jsxBindings, rawBindings)) {
      const expression = node.expression.getText(tree);
      if (!exemptions.has(`${file}:${expression}`)) found.push({ file, line: tree.getLineAndCharacterOfPosition(node.getStart()).line + 1, expression });
    }
    ts.forEachChild(node, visit);
  }
  visit(tree);
  return found;
}

export function checkErrorPresentation(root) {
  const found = [];
  function scan(directory) {
    for (const entry of readdirSync(path.join(root, directory), { withFileTypes: true })) {
      const name = `${directory}/${entry.name}`;
      if (entry.isDirectory()) scan(name);
      else if (name.endsWith(".tsx") && !/\.(?:test|testdata|harness)\./.test(name)) found.push(...rawErrorRenders(readFileSync(path.join(root, name), "utf8"), name));
    }
  }
  scan("src/components");
  return found;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const found = checkErrorPresentation(root);
  for (const item of found) process.stderr.write(`${item.file}:${item.line}: raw error JSX: ${item.expression}\n`);
  if (found.length) process.exitCode = 1;
  else process.stdout.write("Error presentation guard: no raw JSX error strings outside shared notices and field validation.\n");
}
