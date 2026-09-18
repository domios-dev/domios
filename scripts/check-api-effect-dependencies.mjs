import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import ts from "typescript";

const root = process.cwd();
const sourceRoot = path.join(root, "src");
const apiCallNames = new Set(["fetch", "invoke", "webFetch", "webJson"]);
const effectHookNames = new Set(["useEffect", "useLayoutEffect"]);

function sourceFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(file);
    return /\.(ts|tsx)$/.test(entry.name) && !/\.test\.[^.]+$/.test(entry.name) ? [file] : [];
  });
}

function isApiCall(node) {
  if (!ts.isCallExpression(node)) return false;
  if (ts.isIdentifier(node.expression)) {
    return apiCallNames.has(node.expression.text) || /^(fetch|refetch|refresh)/u.test(node.expression.text);
  }
  return ts.isPropertyAccessExpression(node.expression) && apiCallNames.has(node.expression.name.text);
}

function reachesApi(effect) {
  let found = false;
  function visit(node) {
    if (found) return;
    if (isApiCall(node)) {
      found = true;
      return;
    }
    ts.forEachChild(node, visit);
  }
  visit(effect.body);
  return found;
}

function isPrimitive(type, checker) {
  if (type.isUnion()) return type.types.every((member) => isPrimitive(member, checker));
  const typeNames = [type.symbol?.name, type.aliasSymbol?.name, checker.typeToString(type)];
  if (typeNames.some((name) => /(?:^|[.< ])(?:MutableRefObject|RefObject)(?:<|$)/u.test(name ?? ""))) return true;
  if (checker.getSignaturesOfType(type, ts.SignatureKind.Call).length > 0) return true;
  return Boolean(
    type.flags & (
      ts.TypeFlags.StringLike |
      ts.TypeFlags.NumberLike |
      ts.TypeFlags.BooleanLike |
      ts.TypeFlags.BigIntLike |
      ts.TypeFlags.ESSymbolLike |
      ts.TypeFlags.Null |
      ts.TypeFlags.Undefined |
      ts.TypeFlags.Void |
      ts.TypeFlags.Never
    ),
  );
}

export function findViolations(files = sourceFiles(sourceRoot)) {
  const options = ts.getDefaultCompilerOptions();
  options.jsx = ts.JsxEmit.ReactJSX;
  const program = ts.createProgram(files, options);
  const checker = program.getTypeChecker();
  const violations = [];

  for (const file of files) {
    const source = program.getSourceFile(file);
    if (!source) continue;
    function visit(node) {
      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(node.expression) &&
        effectHookNames.has(node.expression.text) &&
        node.arguments.length >= 2 &&
        ts.isArrowFunction(node.arguments[0]) &&
        ts.isArrayLiteralExpression(node.arguments[1]) &&
        reachesApi(node.arguments[0])
      ) {
        for (const dependency of node.arguments[1].elements) {
          const type = checker.getTypeAtLocation(dependency);
          if (!isPrimitive(type, checker)) {
            const position = source.getLineAndCharacterOfPosition(dependency.getStart(source));
            violations.push({
              file: path.relative(root, file).replaceAll(path.sep, "/"),
              line: position.line + 1,
              dependency: dependency.getText(source),
              type: checker.typeToString(type),
            });
          }
        }
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
  return violations;
}

const violations = findViolations();
if (violations.length > 0) {
  for (const violation of violations) {
    console.error(`${violation.file}:${violation.line}: API-reaching useEffect has non-primitive dependency ${violation.dependency} (${violation.type})`);
  }
  process.exitCode = 1;
}
